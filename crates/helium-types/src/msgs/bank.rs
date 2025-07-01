//! Bank module message types

use crate::{address::AccAddress, error::SdkError, tx::SdkMsg};
use helium_codec::protobuf::MessageExt;
use helium_math::Coins;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Internal protobuf representation of MsgSend
#[derive(Clone, PartialEq, Eq, Message)]
struct MsgSendProto {
    /// The sender's address as bech32 string
    #[prost(string, tag = "1")]
    pub from_address: String,
    /// The recipient's address as bech32 string
    #[prost(string, tag = "2")]
    pub to_address: String,
    /// The amount to send as coin protobuf messages
    #[prost(message, repeated, tag = "3")]
    pub amount: Vec<CoinProto>,
}

/// Internal protobuf representation of Coin
#[derive(Clone, PartialEq, Eq, Message)]
struct CoinProto {
    /// The denomination
    #[prost(string, tag = "1")]
    pub denom: String,
    /// The amount as string
    #[prost(string, tag = "2")]
    pub amount: String,
}

/// MsgSend represents a message to send coins from one account to another
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgSend {
    /// The sender's address
    pub from_address: AccAddress,
    /// The recipient's address  
    pub to_address: AccAddress,
    /// The amount to send
    pub amount: Coins,
}

impl MsgSend {
    /// Create a new MsgSend
    pub fn new(from_address: AccAddress, to_address: AccAddress, amount: Coins) -> Self {
        Self {
            from_address,
            to_address,
            amount,
        }
    }
}

impl SdkMsg for MsgSend {
    fn type_url(&self) -> &'static str {
        "/cosmos.bank.v1beta1.MsgSend"
    }

    fn validate_basic(&self) -> Result<(), SdkError> {
        // Check that addresses are different
        if self.from_address == self.to_address {
            return Err(SdkError::InvalidRequest(
                "cannot send coins to the same address".to_string(),
            ));
        }

        // Check that amount is not empty
        if self.amount.is_empty() {
            return Err(SdkError::InvalidRequest(
                "amount cannot be empty".to_string(),
            ));
        }

        // Check that all amounts are positive (non-zero)
        for coin in self.amount.as_slice() {
            if coin.is_zero() {
                return Err(SdkError::InvalidRequest(format!(
                    "amount cannot be zero for denom {}",
                    coin.denom
                )));
            }
        }

        Ok(())
    }

    fn get_signers(&self) -> Result<Vec<AccAddress>, SdkError> {
        // Only the sender needs to sign
        Ok(vec![self.from_address])
    }

    fn encode(&self) -> Vec<u8> {
        use prost::Message;

        // Convert to protobuf representation
        let proto_coins: Vec<CoinProto> = self
            .amount
            .as_slice()
            .iter()
            .map(|coin| CoinProto {
                denom: coin.denom.clone(),
                amount: coin.amount.to_string(),
            })
            .collect();

        let proto_msg = MsgSendProto {
            from_address: self.from_address.to_string(),
            to_address: self.to_address.to_string(),
            amount: proto_coins,
        };

        let mut buf = Vec::new();
        proto_msg.encode(&mut buf).unwrap_or_default();
        buf
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl MessageExt for MsgSend {
    const TYPE_URL: &'static str = "/cosmos.bank.v1beta1.MsgSend";

    fn type_url(&self) -> &'static str {
        // Delegate to SdkMsg implementation
        <Self as SdkMsg>::type_url(self)
    }
}

impl Message for MsgSend {
    fn encode_raw<B>(&self, buf: &mut B)
    where
        B: prost::bytes::BufMut,
        Self: Sized,
    {
        // Convert to protobuf representation first
        let proto_coins: Vec<CoinProto> = self
            .amount
            .as_slice()
            .iter()
            .map(|coin| CoinProto {
                denom: coin.denom.clone(),
                amount: coin.amount.to_string(),
            })
            .collect();

        let proto_msg = MsgSendProto {
            from_address: self.from_address.to_string(),
            to_address: self.to_address.to_string(),
            amount: proto_coins,
        };

        proto_msg.encode_raw(buf);
    }

    fn merge_field<B>(
        &mut self,
        tag: u32,
        wire_type: prost::encoding::WireType,
        buf: &mut B,
        ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError>
    where
        B: prost::bytes::Buf,
        Self: Sized,
    {
        // For decoding, we'll create a proto message first and then convert
        let mut proto_msg = MsgSendProto::default();
        proto_msg.merge_field(tag, wire_type, buf, ctx)?;

        // Convert from proto representation
        *self = MsgSend {
            from_address: proto_msg
                .from_address
                .parse()
                .map_err(|_| prost::DecodeError::new("invalid from_address"))?,
            to_address: proto_msg
                .to_address
                .parse()
                .map_err(|_| prost::DecodeError::new("invalid to_address"))?,
            amount: {
                use helium_math::{Coin, Int};
                let coins: Result<Vec<Coin>, _> = proto_msg
                    .amount
                    .into_iter()
                    .map(|proto_coin| {
                        let amount = proto_coin
                            .amount
                            .parse::<u64>()
                            .map_err(|_| prost::DecodeError::new("invalid coin amount"))
                            .and_then(|val| Ok(Int::from_u64(val)))
                            .map_err(|_| prost::DecodeError::new("invalid coin amount"))?;
                        Coin::new(proto_coin.denom, amount)
                            .map_err(|_| prost::DecodeError::new("invalid coin"))
                    })
                    .collect();
                Coins::new(coins?).map_err(|_| prost::DecodeError::new("invalid coins"))?
            },
        };

        Ok(())
    }

    fn encoded_len(&self) -> usize {
        // Convert to protobuf representation to get length
        let proto_coins: Vec<CoinProto> = self
            .amount
            .as_slice()
            .iter()
            .map(|coin| CoinProto {
                denom: coin.denom.clone(),
                amount: coin.amount.to_string(),
            })
            .collect();

        let proto_msg = MsgSendProto {
            from_address: self.from_address.to_string(),
            to_address: self.to_address.to_string(),
            amount: proto_coins,
        };

        proto_msg.encoded_len()
    }

    fn clear(&mut self) {
        // Reset to default values
        *self = MsgSend::default();
    }
}

impl Default for MsgSend {
    fn default() -> Self {
        use helium_math::Coins;
        Self {
            from_address: AccAddress::default(),
            to_address: AccAddress::default(),
            amount: Coins::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helium_math::{Coin, Int};

    #[test]
    fn test_msg_send_validation_basic() {
        // Create test addresses using from_pubkey
        let from_pubkey = [1u8; 33]; // Mock public key
        let to_pubkey = [2u8; 33]; // Different mock public key
        let from_addr = AccAddress::from_pubkey(&from_pubkey);
        let to_addr = AccAddress::from_pubkey(&to_pubkey);

        // Valid case
        let coin = Coin::new("uatom".to_string(), Int::from_u64(100)).unwrap();
        let coins = Coins::new(vec![coin]).unwrap();
        let msg = MsgSend::new(from_addr, to_addr, coins);
        assert!(msg.validate_basic().is_ok());

        // Same address case
        let coin = Coin::new("uatom".to_string(), Int::from_u64(100)).unwrap();
        let coins = Coins::new(vec![coin]).unwrap();
        let msg = MsgSend::new(from_addr, from_addr, coins);
        assert!(msg.validate_basic().is_err());

        // Empty amount case
        let coins = Coins::empty();
        let msg = MsgSend::new(from_addr, to_addr, coins);
        assert!(msg.validate_basic().is_err());
    }

    #[test]
    fn test_msg_send_signers() {
        let from_pubkey = [1u8; 33];
        let to_pubkey = [2u8; 33];
        let from_addr = AccAddress::from_pubkey(&from_pubkey);
        let to_addr = AccAddress::from_pubkey(&to_pubkey);
        let coin = Coin::new("uatom".to_string(), Int::from_u64(100)).unwrap();
        let coins = Coins::new(vec![coin]).unwrap();

        let msg = MsgSend::new(from_addr, to_addr, coins);
        let signers = msg.get_signers().unwrap();

        assert_eq!(signers.len(), 1);
        assert_eq!(signers[0], from_addr);
    }

    #[test]
    fn test_msg_send_type_url() {
        let from_pubkey = [1u8; 33];
        let to_pubkey = [2u8; 33];
        let from_addr = AccAddress::from_pubkey(&from_pubkey);
        let to_addr = AccAddress::from_pubkey(&to_pubkey);
        let coin = Coin::new("uatom".to_string(), Int::from_u64(100)).unwrap();
        let coins = Coins::new(vec![coin]).unwrap();

        let msg = MsgSend::new(from_addr, to_addr, coins);
        assert_eq!(
            <MsgSend as SdkMsg>::type_url(&msg),
            "/cosmos.bank.v1beta1.MsgSend"
        );
    }
}
