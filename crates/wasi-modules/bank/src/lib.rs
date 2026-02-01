mod bindings;
use bindings::exports::gridway::framework::module::{
    Event, EventAttribute, Guest, Message, ModuleContext, ModuleResponse,
};
use bindings::gridway::framework::kvstore;

struct BankModule;

impl Guest for BankModule {
    fn handle(context: ModuleContext, msg: Message) -> ModuleResponse {
        match msg.type_url.as_str() {
            "/cosmos.bank.v1beta1.MsgSend" | "/gridway.bank.v1.MsgSend" => {
                handle_msg_send(&context, &msg)
            }
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
    // Parse MsgSend from JSON — supports both Cosmos and flat formats
    let raw: serde_json::Value = match serde_json::from_str(&msg.data) {
        Ok(v) => v,
        Err(e) => return error_response(format!("invalid MsgSend JSON: {}", e)),
    };

    let from_address = match raw.get("from_address").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return error_response("missing from_address".into()),
    };
    let to_address = match raw.get("to_address").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return error_response("missing to_address".into()),
    };

    // Parse amount — supports both formats:
    // Cosmos: "amount": [{"denom": "ugridway", "amount": "100"}]
    // Flat:   "amount": 100, "denom": "ugridway"
    let (amount, denom) = if let Some(arr) = raw.get("amount").and_then(|v| v.as_array()) {
        // Cosmos format
        if arr.is_empty() {
            return error_response("empty amount array".into());
        }
        let coin = &arr[0];
        let amt_str = coin.get("amount").and_then(|v| v.as_str()).unwrap_or("0");
        let amt: u128 = match amt_str.parse() {
            Ok(a) => a,
            Err(_) => return error_response(format!("invalid amount: {}", amt_str)),
        };
        let d = coin.get("denom").and_then(|v| v.as_str()).unwrap_or("").to_string();
        (amt, d)
    } else if let Some(amt_val) = raw.get("amount") {
        // Flat format
        let amt: u128 = if let Some(n) = amt_val.as_u64() {
            n as u128
        } else if let Some(s) = amt_val.as_str() {
            match s.parse() {
                Ok(a) => a,
                Err(_) => return error_response(format!("invalid amount: {}", s)),
            }
        } else {
            return error_response("invalid amount type".into());
        };
        let d = raw.get("denom").and_then(|v| v.as_str()).unwrap_or("").to_string();
        (amt, d)
    } else {
        return error_response("missing amount".into());
    };

    if denom.is_empty() {
        return error_response("missing denom".into());
    }
    if amount == 0 {
        return error_response("zero amount".into());
    }

    // Open bank store via kvstore
    let store = match kvstore::open_store("bank") {
        Ok(s) => s,
        Err(e) => return error_response(format!("failed to open bank store: {}", e)),
    };

    // Check sender balance
    let sender_key = format!("balance_{}_{}", from_address, denom);
    let sender_balance = get_balance(&store, &sender_key);

    if sender_balance < amount {
        return error_response(format!(
            "insufficient funds: {} < {}",
            sender_balance, amount
        ));
    }

    // Deduct from sender
    let new_sender = sender_balance - amount;
    store.set(sender_key.as_bytes(), new_sender.to_string().as_bytes());

    // Add to recipient
    let recipient_key = format!("balance_{}_{}", to_address, denom);
    let recipient_balance = get_balance(&store, &recipient_key);
    let new_recipient = recipient_balance + amount;
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
                    value: from_address,
                },
                EventAttribute {
                    key: "recipient".to_string(),
                    value: to_address,
                },
                EventAttribute {
                    key: "amount".to_string(),
                    value: format!("{}{}", amount, denom),
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
