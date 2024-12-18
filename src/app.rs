//! Main application module for the P2P trading system.
//! Handles message routing, action processing, and event loop management.

// Submodules for different trading actions
pub mod add_invoice; // Handles invoice creation
pub mod admin_add_solver; // Admin functionality to add dispute solvers
pub mod admin_cancel; // Admin order cancellation
pub mod admin_settle; // Admin dispute settlement
pub mod admin_take_dispute; // Admin dispute handling
pub mod cancel; // User order cancellation
pub mod dispute; // User dispute handling
pub mod fiat_sent; // Fiat payment confirmation
pub mod order; // Order creation and management
pub mod rate_user; // User reputation system
pub mod release; // Release of held funds
pub mod take_buy; // Taking buy orders
pub mod take_sell; // Taking sell orders

// Import action handlers from submodules
use crate::app::add_invoice::add_invoice_action;
use crate::app::admin_add_solver::admin_add_solver_action;
use crate::app::admin_cancel::admin_cancel_action;
use crate::app::admin_settle::admin_settle_action;
use crate::app::admin_take_dispute::admin_take_dispute_action;
use crate::app::cancel::cancel_action;
use crate::app::dispute::dispute_action;
use crate::app::fiat_sent::fiat_sent_action;
use crate::app::order::order_action;
use crate::app::rate_user::update_user_reputation_action;
use crate::app::release::release_action;
use crate::app::take_buy::take_buy_action;
use crate::app::take_sell::take_sell_action;

// Core functionality imports
use crate::lightning::LndConnector;
use crate::nip59::unwrap_gift_wrap;
use crate::Settings;

// External dependencies
use anyhow::Result;
use mostro_core::message::{Action, Message};
use nostr_sdk::prelude::*;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Helper function to log warning messages for action errors
fn warning_msg(action: &Action, e: anyhow::Error) {
    tracing::warn!("Error in {} with context {}", action, e);
}

/// Handles the processing of a single message action by routing it to the appropriate handler
/// based on the action type. This is the core message routing logic of the application.
///
/// # Arguments
/// * `action` - The type of action to be performed
/// * `msg` - The message containing action details
/// * `event` - The unwrapped gift wrap event
/// * `my_keys` - Node keypair for signing/verification
/// * `pool` - Database connection pool
/// * `ln_client` - Lightning network connector
/// * `rate_list` - Shared list of rating events
async fn handle_message_action(
    action: &Action,
    msg: Message,
    event: &UnwrappedGift,
    my_keys: &Keys,
    pool: &Pool<Sqlite>,
    ln_client: &mut LndConnector,
    rate_list: Arc<Mutex<Vec<Event>>>,
) -> Result<()> {
    match action {
        // Order-related actions
        Action::NewOrder => order_action(msg, event, my_keys, pool).await,
        Action::TakeSell => take_sell_action(msg, event, my_keys, pool).await,
        Action::TakeBuy => take_buy_action(msg, event, my_keys, pool).await,

        // Payment-related actions
        Action::FiatSent => fiat_sent_action(msg, event, my_keys, pool).await,
        Action::Release => release_action(msg, event, my_keys, pool, ln_client).await,
        Action::AddInvoice => add_invoice_action(msg, event, my_keys, pool).await,
        Action::PayInvoice => todo!(),

        // Dispute and rating actions
        Action::Dispute => dispute_action(msg, event, my_keys, pool).await,
        Action::RateUser => {
            update_user_reputation_action(msg, event, my_keys, pool, rate_list).await
        }
        Action::Cancel => cancel_action(msg, event, my_keys, pool, ln_client).await,

        // Admin actions
        Action::AdminCancel => admin_cancel_action(msg, event, my_keys, pool, ln_client).await,
        Action::AdminSettle => admin_settle_action(msg, event, my_keys, pool, ln_client).await,
        Action::AdminAddSolver => admin_add_solver_action(msg, event, my_keys, pool).await,
        Action::AdminTakeDispute => admin_take_dispute_action(msg, event, pool).await,

        _ => {
            tracing::info!("Received message with action {:?}", action);
            Ok(())
        }
    }
}

/// Main event loop that processes incoming Nostr events.
/// Handles message verification, POW checking, and routes valid messages to appropriate handlers.
///
/// # Arguments
/// * `my_keys` - The node's keypair
/// * `client` - Nostr client instance
/// * `ln_client` - Lightning network connector
/// * `pool` - SQLite connection pool
/// * `rate_list` - Shared list of rating events
pub async fn run(
    my_keys: Keys,
    client: &Client,
    ln_client: &mut LndConnector,
    pool: Pool<Sqlite>,
    rate_list: Arc<Mutex<Vec<Event>>>,
) -> Result<()> {
    loop {
        let mut notifications = client.notifications();

        // Get pow from config
        let pow = Settings::get_mostro().pow;
        while let Ok(notification) = notifications.recv().await {
            if let RelayPoolNotification::Event { event, .. } = notification {
                // Verify proof of work
                if !event.check_pow(pow) {
                    // Discard events that don't meet POW requirements
                    tracing::info!("Not POW verified event!");
                    continue;
                }
                if let Kind::GiftWrap = event.kind {
                    // Validate event signature
                    if event.verify().is_err() {
                        tracing::warn!("Error in event verification")
                    };

                    let event = unwrap_gift_wrap(&my_keys, &event)?;
                    // Discard events older than 10 seconds to prevent replay attacks
                    let since_time = chrono::Utc::now()
                        .checked_sub_signed(chrono::Duration::seconds(10))
                        .unwrap()
                        .timestamp() as u64;
                    if event.rumor.created_at.as_u64() < since_time {
                        continue;
                    }

                    // Parse and process the message
                    let message = Message::from_json(&event.rumor.content);
                    match message {
                        Ok(msg) => {
                            if msg.get_inner_message_kind().verify() {
                                if let Some(action) = msg.inner_action() {
                                    if let Err(e) = handle_message_action(
                                        &action,
                                        msg,
                                        &event,
                                        &my_keys,
                                        &pool,
                                        ln_client,
                                        rate_list.clone(),
                                    )
                                    .await
                                    {
                                        warning_msg(&action, e)
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse event message from JSON: {:?}", e)
                        }
                    }
                }
            }
        }
    }
}
