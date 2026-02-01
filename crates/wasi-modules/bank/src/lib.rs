mod bindings;
use bindings::exports::gridway::framework::module::{
    Event, EventAttribute, Guest, Message, ModuleContext, ModuleResponse,
};
use bindings::gridway::framework::kvstore;

struct BankModule;

impl Guest for BankModule {
    fn handle(context: ModuleContext, msg: Message) -> ModuleResponse {
        match msg.type_url.as_str() {
            "/cosmos.bank.v1beta1.MsgSend" => handle_msg_send(&context, &msg),
            _ => ModuleResponse {
                success: false,
                data: None,
                events: vec![],
                error: Some(format!("unknown message type: {}", msg.type_url)),
                gas_used: 0,
            },
        }
    }

    fn query(path: String, data: Vec<u8>) -> Result<Vec<u8>, String> {
        match path.as_str() {
            "balance" => query_balance(&data),
            _ => Err(format!("unknown query: {}", path)),
        }
    }
}

fn handle_msg_send(_ctx: &ModuleContext, msg: &Message) -> ModuleResponse {
    // Parse MsgSend from JSON
    let send: MsgSend = match serde_json::from_str(&msg.data) {
        Ok(s) => s,
        Err(e) => return error_response(format!("invalid MsgSend: {}", e)),
    };

    // Open bank store via kvstore
    let store = match kvstore::open_store("bank") {
        Ok(s) => s,
        Err(e) => return error_response(format!("failed to open bank store: {}", e)),
    };

    // Check sender balance
    let sender_key = format!("balance_{}_{}", send.from_address, send.denom);
    let sender_balance = get_balance(&store, &sender_key);

    if sender_balance < send.amount {
        return error_response(format!(
            "insufficient funds: {} < {}",
            sender_balance, send.amount
        ));
    }

    // Deduct from sender
    let new_sender = sender_balance - send.amount;
    store.set(sender_key.as_bytes(), new_sender.to_string().as_bytes());

    // Add to recipient
    let recipient_key = format!("balance_{}_{}", send.to_address, send.denom);
    let recipient_balance = get_balance(&store, &recipient_key);
    let new_recipient = recipient_balance + send.amount;
    store.set(
        recipient_key.as_bytes(),
        new_recipient.to_string().as_bytes(),
    );

    ModuleResponse {
        success: true,
        data: None,
        events: vec![Event {
            event_type: "transfer".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "sender".to_string(),
                    value: send.from_address,
                },
                EventAttribute {
                    key: "recipient".to_string(),
                    value: send.to_address,
                },
                EventAttribute {
                    key: "amount".to_string(),
                    value: format!("{}{}", send.amount, send.denom),
                },
            ],
        }],
        error: None,
        gas_used: 65000,
    }
}

fn get_balance(store: &kvstore::Store, key: &str) -> u128 {
    store
        .get(key.as_bytes())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn query_balance(data: &[u8]) -> Result<Vec<u8>, String> {
    let query: BalanceQuery =
        serde_json::from_slice(data).map_err(|e| format!("invalid query: {}", e))?;
    let store =
        kvstore::open_store("bank").map_err(|e| format!("failed to open store: {}", e))?;
    let key = format!("balance_{}_{}", query.address, query.denom);
    let balance = get_balance(&store, &key);
    serde_json::to_vec(&BalanceResponse {
        balance: balance.to_string(),
        denom: query.denom,
    })
    .map_err(|e| format!("serialization error: {}", e))
}

fn error_response(msg: String) -> ModuleResponse {
    ModuleResponse {
        success: false,
        data: None,
        events: vec![],
        error: Some(msg),
        gas_used: 0,
    }
}

#[derive(serde::Deserialize)]
struct MsgSend {
    from_address: String,
    to_address: String,
    amount: u128,
    denom: String,
}

#[derive(serde::Deserialize)]
struct BalanceQuery {
    address: String,
    denom: String,
}

#[derive(serde::Serialize)]
struct BalanceResponse {
    balance: String,
    denom: String,
}

bindings::export!(BankModule with_types_in bindings);
