use crate::db::{
    edit_buyer_pubkey_order, edit_seller_pubkey_order, find_order_by_id,
    update_order_to_initial_state,
};
use crate::lightning::LndConnector;
use crate::util::{send_cant_do_msg, send_new_order_msg, update_order_event};

use anyhow::{Error, Result};
use mostro_core::message::{Action, CantDoReason, Message};
use mostro_core::order::{Kind as OrderKind, Order, Status};
use nostr::nips::nip59::UnwrappedGift;
use nostr_sdk::prelude::*;
use sqlx::{Pool, Sqlite};
use sqlx_crud::Crud;
use std::str::FromStr;
use tracing::{error, info};

pub async fn cancel_action(
    msg: Message,
    event: &UnwrappedGift,
    my_keys: &Keys,
    pool: &Pool<Sqlite>,
    ln_client: &mut LndConnector,
) -> Result<()> {
    // Get request id
    let request_id = msg.get_inner_message_kind().request_id;

    let order_id = if let Some(order_id) = msg.get_inner_message_kind().id {
        order_id
    } else {
        return Err(Error::msg("No order id"));
    };
    let user_pubkey = event.rumor.pubkey.to_string();

    let mut order = match find_order_by_id(pool, order_id, &user_pubkey).await {
        Ok(order) => order,
        Err(_) => {
            error!("Order Id {order_id} not found for user with pubkey: {user_pubkey}");
            return Ok(());
        }
    };

    if order.status == Status::Canceled.to_string()
        || order.status == Status::CooperativelyCanceled.to_string()
        || order.status == Status::CanceledByAdmin.to_string()
    {
        send_cant_do_msg(
            request_id,
            Some(order_id),
            Some(CantDoReason::OrderAlreadyCanceled),
            &event.rumor.pubkey,
        )
        .await;
        return Ok(());
    }

    if order.status == Status::Pending.to_string() {
        // Validates if this user is the order creator
        if user_pubkey != order.creator_pubkey {
            send_cant_do_msg(
                request_id,
                Some(order.id),
                Some(CantDoReason::IsNotYourOrder),
                &event.rumor.pubkey,
            )
            .await;
        } else {
            // We publish a new replaceable kind nostr event with the status updated
            // and update on local database the status and new event id
            if let Ok(order_updated) = update_order_event(my_keys, Status::Canceled, &order).await {
                let _ = order_updated.update(pool).await;
            }
            // We create a Message for cancel
            send_new_order_msg(
                request_id,
                Some(order.id),
                Action::Canceled,
                None,
                &event.rumor.pubkey,
                None,
            )
            .await;
        }

        return Ok(());
    }

    if order.kind == OrderKind::Sell.to_string()
        && order.status == Status::WaitingBuyerInvoice.to_string()
    {
        cancel_add_invoice(ln_client, &mut order, event, pool, my_keys, request_id).await?;
    }

    if order.kind == OrderKind::Buy.to_string()
        && order.status == Status::WaitingPayment.to_string()
    {
        cancel_pay_hold_invoice(ln_client, &mut order, event, pool, my_keys, request_id).await?;
    }

    if order.status == Status::Active.to_string()
        || order.status == Status::FiatSent.to_string()
        || order.status == Status::Dispute.to_string()
    {
        let (seller_pubkey, buyer_pubkey) = match (&order.seller_pubkey, &order.buyer_pubkey) {
            (Some(seller), Some(buyer)) => (seller, buyer),
            (None, _) => return Err(Error::msg("Missing seller pubkey")),
            (_, None) => return Err(Error::msg("Missing buyer pubkey")),
        };

        let counterparty_pubkey: String;
        if buyer_pubkey == &user_pubkey {
            order.buyer_cooperativecancel = true;
            counterparty_pubkey = seller_pubkey.to_string();
        } else {
            order.seller_cooperativecancel = true;
            counterparty_pubkey = buyer_pubkey.to_string();
        }

        match order.cancel_initiator_pubkey {
            Some(ref initiator_pubkey) => {
                if initiator_pubkey == &user_pubkey {
                    // We create a Message
                    send_cant_do_msg(request_id, Some(order_id), None, &event.rumor.pubkey).await;
                    return Ok(());
                } else {
                    if let Some(hash) = &order.hash {
                        // We return funds to seller
                        ln_client.cancel_hold_invoice(hash).await?;
                        info!(
                            "Cooperative cancel: Order Id {}: Funds returned to seller",
                            &order.id
                        );
                    }
                    order.status = Status::CooperativelyCanceled.to_string();
                    // update db
                    let order = order.update(pool).await?;
                    // We publish a new replaceable kind nostr event with the status updated
                    // and update on local database the status and new event id
                    update_order_event(my_keys, Status::CooperativelyCanceled, &order).await?;
                    // We create a Message for an accepted cooperative cancel and send it to both parties
                    send_new_order_msg(
                        request_id,
                        Some(order.id),
                        Action::CooperativeCancelAccepted,
                        None,
                        &event.rumor.pubkey,
                        None,
                    )
                    .await;
                    let counterparty_pubkey = PublicKey::from_str(&counterparty_pubkey)?;
                    send_new_order_msg(
                        None,
                        Some(order.id),
                        Action::CooperativeCancelAccepted,
                        None,
                        &counterparty_pubkey,
                        None,
                    )
                    .await;
                    info!("Cancel: Order Id {order_id} canceled cooperatively!");
                }
            }
            None => {
                order.cancel_initiator_pubkey = Some(user_pubkey.clone());
                // update db
                let order = order.update(pool).await?;
                // We create a Message to start a cooperative cancel and send it to both parties
                send_new_order_msg(
                    request_id,
                    Some(order.id),
                    Action::CooperativeCancelInitiatedByYou,
                    None,
                    &event.rumor.pubkey,
                    None,
                )
                .await;
                let counterparty_pubkey = PublicKey::from_str(&counterparty_pubkey)?;
                send_new_order_msg(
                    None,
                    Some(order.id),
                    Action::CooperativeCancelInitiatedByPeer,
                    None,
                    &counterparty_pubkey,
                    None,
                )
                .await;
            }
        }
    }
    Ok(())
}

pub async fn cancel_add_invoice(
    ln_client: &mut LndConnector,
    order: &mut Order,
    event: &UnwrappedGift,
    pool: &Pool<Sqlite>,
    my_keys: &Keys,
    request_id: Option<u64>,
) -> Result<()> {
    if let Some(hash) = &order.hash {
        ln_client.cancel_hold_invoice(hash).await?;
        info!("Order Id {}: Funds returned to seller", &order.id);
    }

    let user_pubkey = event.rumor.pubkey.to_string();

    let (seller_pubkey, buyer_pubkey) = match (&order.seller_pubkey, &order.buyer_pubkey) {
        (Some(seller), Some(buyer)) => (PublicKey::from_str(seller.as_str())?, buyer),
        (None, _) => return Err(Error::msg("Missing seller pubkey")),
        (_, None) => return Err(Error::msg("Missing buyer pubkey")),
    };

    if buyer_pubkey != &user_pubkey {
        // We create a Message
        send_cant_do_msg(request_id, Some(order.id), None, &event.rumor.pubkey).await;
        return Ok(());
    }

    if &order.creator_pubkey == buyer_pubkey {
        // We publish a new replaceable kind nostr event with the status updated
        // and update on local database the status and new event id
        update_order_event(my_keys, Status::CooperativelyCanceled, order).await?;
        // We create a Message for cancel
        send_new_order_msg(
            request_id,
            Some(order.id),
            Action::Canceled,
            None,
            &event.rumor.pubkey,
            None,
        )
        .await;
        send_new_order_msg(
            None,
            Some(order.id),
            Action::Canceled,
            None,
            &seller_pubkey,
            None,
        )
        .await;
        Ok(())
    } else {
        // We re-publish the event with Pending status
        // and update on local database
        if order.price_from_api {
            order.amount = 0;
            order.fee = 0;
        }
        edit_buyer_pubkey_order(pool, order.id, None).await?;
        update_order_to_initial_state(pool, order.id, order.amount, order.fee).await?;
        update_order_event(my_keys, Status::Pending, order).await?;
        info!(
            "{}: Canceled order Id {} republishing order",
            buyer_pubkey, order.id
        );
        // Confirmation message to buyer
        send_new_order_msg(
            request_id,
            Some(order.id),
            Action::Canceled,
            None,
            &event.rumor.pubkey,
            None,
        )
        .await;
        Ok(())
    }
}

pub async fn cancel_pay_hold_invoice(
    ln_client: &mut LndConnector,
    order: &mut Order,
    event: &UnwrappedGift,
    pool: &Pool<Sqlite>,
    my_keys: &Keys,
    request_id: Option<u64>,
) -> Result<()> {
    let user_pubkey = event.rumor.pubkey.to_string();

    let (seller_pubkey, buyer_pubkey) = match (&order.seller_pubkey, &order.buyer_pubkey) {
        (Some(seller), Some(buyer)) => (PublicKey::from_str(seller.as_str())?, buyer),
        (None, _) => return Err(Error::msg("Missing seller pubkey")),
        (_, None) => return Err(Error::msg("Missing buyer pubkey")),
    };

    if seller_pubkey.to_string() != user_pubkey {
        // We create a Message
        send_cant_do_msg(request_id, Some(order.id), None, &event.rumor.pubkey).await;
        return Ok(());
    }

    if order.hash.is_some() {
        // We cancel the hold invoice, if it was paid those funds return to seller
        if let Some(hash) = order.hash.as_ref() {
            ln_client.cancel_hold_invoice(hash).await?;
            info!("Order Id {}: Hold invoice canceled", &order.id);
        }
    }

    if order.creator_pubkey == seller_pubkey.to_string() {
        // We publish a new replaceable kind nostr event with the status updated
        // and update on local database the status and new event id
        update_order_event(my_keys, Status::Canceled, order).await?;
        // We create a Message for cancel
        send_new_order_msg(
            request_id,
            Some(order.id),
            Action::Canceled,
            None,
            &event.rumor.pubkey,
            None,
        )
        .await;
        send_new_order_msg(
            None,
            Some(order.id),
            Action::Canceled,
            None,
            &seller_pubkey,
            None,
        )
        .await;
        Ok(())
    } else {
        // We re-publish the event with Pending status
        // and update on local database
        if order.price_from_api {
            order.amount = 0;
            order.fee = 0;
        }
        edit_seller_pubkey_order(pool, order.id, None).await?;
        update_order_to_initial_state(pool, order.id, order.amount, order.fee).await?;
        update_order_event(my_keys, Status::Pending, order).await?;
        info!(
            "{}: Canceled order Id {} republishing order",
            buyer_pubkey, order.id
        );
        // Notify to seller the order was canceled
        send_new_order_msg(
            request_id,
            Some(order.id),
            Action::Canceled,
            None,
            &event.rumor.pubkey,
            None,
        )
        .await;
        Ok(())
    }
}
