# release

After confirming the buyer sent the fiat money, the seller should send a message to Mostro indicating that sats should be delivered to the buyer, the message will look like this:

```json
{
  "order": {
    "version": 1,
    "id": "ede61c96-4c13-4519-bf3a-dcf7f1e9d842",
    "pubkey": "00000ba40c5795451705bb9c165b3af93c846894d3062a9cd7fcba090eb3bf78",
    "action": "release",
    "content": null
  }
}
```

## Mostro response

Here an example of the Mostro response to the seller:

```json
{
  "order": {
    "version": 1,
    "id": "ede61c96-4c13-4519-bf3a-dcf7f1e9d842",
    "pubkey": null,
    "action": "hold-invoice-payment-settled",
    "content": null
  }
}
```

And a message to the buyer to let him know that the sats were released:

```json
{
  "order": {
    "version": 1,
    "id": "ede61c96-4c13-4519-bf3a-dcf7f1e9d842",
    "pubkey": null,
    "action": "released",
    "content": null
  }
}
```

## Buyer receives sats

Right after seller release sats Mostro will try to pay the buyer's lightning invoice, if the payment is successful Mostro will send a message to the buyer indicating that the purchase was completed:

```json
{
  "order": {
    "version": 1,
    "id": "ede61c96-4c13-4519-bf3a-dcf7f1e9d842",
    "pubkey": null,
    "action": "purchase-completed",
    "content": null
  }
}
```

Mostro updates the parameterized replaceable event with `d` tag `ede61c96-4c13-4519-bf3a-dcf7f1e9d842` to change the status to `settled-hold-invoice`:

```json
[
  "EVENT",
  "RAND",
  {
    "id": "eb0582360ebd3836c90711f774fbecb27e600f4a5fedf4fc2d16fc852f8380b1",
    "pubkey": "dbe0b1be7aafd3cfba92d7463edbd4e33b2969f61bd554d37ac56f032e13355a",
    "created_at": 1702549437,
    "kind": 38383,
    "tags": [
      ["d", "ede61c96-4c13-4519-bf3a-dcf7f1e9d842"],
      ["k", "sell"],
      ["f", "VES"],
      ["s", "settled-hold-invoice"],
      ["amt", "7851"],
      ["fa", "100"],
      ["pm", "face to face"],
      ["premium", "1"],
      ["y", "mostrop2p"],
      ["z", "order"]
    ],
    "content": "",
    "sig": "a835f8620db3ebdd9fa142ae99c599a61da86321c60f7c9fed0cc57169950f4121757ff64a5e998baccf6b68272aa51819c3e688d8ad586c0177b3cd1ab09c0f"
  }
]
```

Seconds later Mostro will try to pay the buyer's invoice, if the payment is successful Mostro updates the parameterized replaceable event with `d` tag `ede61c96-4c13-4519-bf3a-dcf7f1e9d842` to change the status to `success`:

```json
[
  "EVENT",
  "RAND",
  {
    "id": "eb0582360ebd3836c90711f774fbecb27e600f4a5fedf4fc2d16fc852f8380b1",
    "pubkey": "dbe0b1be7aafd3cfba92d7463edbd4e33b2969f61bd554d37ac56f032e13355a",
    "created_at": 1702549437,
    "kind": 38383,
    "tags": [
      ["d", "ede61c96-4c13-4519-bf3a-dcf7f1e9d842"],
      ["k", "sell"],
      ["f", "VES"],
      ["s", "success"],
      ["amt", "7851"],
      ["fa", "100"],
      ["pm", "face to face"],
      ["premium", "1"],
      ["network", "mainnet"],
      ["layer", "lightning"],
      ["expiration", "1719391096"],
      ["y", "mostrop2p"],
      ["z", "order"]
    ],
    "content": "",
    "sig": "a835f8620db3ebdd9fa142ae99c599a61da86321c60f7c9fed0cc57169950f4121757ff64a5e998baccf6b68272aa51819c3e688d8ad586c0177b3cd1ab09c0f"
  }
]
```
