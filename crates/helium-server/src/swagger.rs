//! Swagger/OpenAPI endpoint for Cosmos SDK compatibility

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// OpenAPI 2.0 (Swagger) specification
#[derive(Debug, Serialize, Deserialize)]
pub struct SwaggerSpec {
    pub swagger: String,
    pub info: SwaggerInfo,
    pub host: String,
    pub base_path: String,
    pub schemes: Vec<String>,
    pub consumes: Vec<String>,
    pub produces: Vec<String>,
    pub paths: serde_json::Value,
    pub definitions: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SwaggerInfo {
    pub title: String,
    pub description: String,
    pub version: String,
}

/// Generate OpenAPI specification for Helium
pub fn generate_swagger_spec() -> SwaggerSpec {
    SwaggerSpec {
        swagger: "2.0".to_string(),
        info: SwaggerInfo {
            title: "Helium REST API".to_string(),
            description: "REST API for Helium blockchain (Cosmos SDK compatible)".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        host: "localhost:1317".to_string(),
        base_path: "/".to_string(),
        schemes: vec!["http".to_string(), "https".to_string()],
        consumes: vec!["application/json".to_string()],
        produces: vec!["application/json".to_string()],
        paths: json!({
            "/health": {
                "get": {
                    "summary": "Health check endpoint",
                    "description": "Returns the health status of the node",
                    "operationId": "getHealth",
                    "responses": {
                        "200": {
                            "description": "Node is healthy",
                            "schema": {
                                "$ref": "#/definitions/HealthResponse"
                            }
                        }
                    }
                }
            },
            "/ready": {
                "get": {
                    "summary": "Readiness check endpoint",
                    "description": "Returns whether the node is ready to serve requests",
                    "operationId": "getReady",
                    "responses": {
                        "200": {
                            "description": "Node readiness status",
                            "schema": {
                                "$ref": "#/definitions/ReadyResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/bank/v1beta1/balances/{address}": {
                "get": {
                    "summary": "Get all balances",
                    "description": "Queries all balances of a single account",
                    "operationId": "getAllBalances",
                    "parameters": [
                        {
                            "name": "address",
                            "in": "path",
                            "required": true,
                            "type": "string",
                            "description": "Account address"
                        },
                        {
                            "name": "pagination.key",
                            "in": "query",
                            "required": false,
                            "type": "string",
                            "format": "byte",
                            "description": "Key for pagination"
                        },
                        {
                            "name": "pagination.limit",
                            "in": "query",
                            "required": false,
                            "type": "string",
                            "format": "uint64",
                            "description": "Maximum number of items"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/QueryAllBalancesResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/bank/v1beta1/balances/{address}/by_denom": {
                "get": {
                    "summary": "Get balance by denomination",
                    "description": "Queries balance of a single coin denomination",
                    "operationId": "getBalance",
                    "parameters": [
                        {
                            "name": "address",
                            "in": "path",
                            "required": true,
                            "type": "string",
                            "description": "Account address"
                        },
                        {
                            "name": "denom",
                            "in": "query",
                            "required": true,
                            "type": "string",
                            "description": "Coin denomination"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/QueryBalanceResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/bank/v1beta1/supply": {
                "get": {
                    "summary": "Get total supply",
                    "description": "Queries the total supply of all coins",
                    "operationId": "getTotalSupply",
                    "parameters": [
                        {
                            "name": "pagination.key",
                            "in": "query",
                            "required": false,
                            "type": "string",
                            "format": "byte"
                        },
                        {
                            "name": "pagination.limit",
                            "in": "query",
                            "required": false,
                            "type": "string",
                            "format": "uint64"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/QueryTotalSupplyResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/auth/v1beta1/accounts/{address}": {
                "get": {
                    "summary": "Get account",
                    "description": "Queries account information",
                    "operationId": "getAccount",
                    "parameters": [
                        {
                            "name": "address",
                            "in": "path",
                            "required": true,
                            "type": "string",
                            "description": "Account address"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/QueryAccountResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/tx/v1beta1/simulate": {
                "post": {
                    "summary": "Simulate transaction",
                    "description": "Simulates executing a transaction to estimate gas",
                    "operationId": "simulateTx",
                    "parameters": [
                        {
                            "name": "body",
                            "in": "body",
                            "required": true,
                            "schema": {
                                "$ref": "#/definitions/SimulateRequest"
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/SimulateResponse"
                            }
                        }
                    }
                }
            },
            "/cosmos/tx/v1beta1/txs": {
                "post": {
                    "summary": "Broadcast transaction",
                    "description": "Broadcasts a signed transaction",
                    "operationId": "broadcastTx",
                    "parameters": [
                        {
                            "name": "body",
                            "in": "body",
                            "required": true,
                            "schema": {
                                "$ref": "#/definitions/BroadcastTxRequest"
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "schema": {
                                "$ref": "#/definitions/BroadcastTxResponse"
                            }
                        }
                    }
                }
            }
        }),
        definitions: json!({
            "HealthResponse": {
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "version": { "type": "string" },
                    "chain_id": { "type": "string" },
                    "block_height": { "type": "string", "format": "uint64" },
                    "abci_connected": { "type": "boolean" },
                    "syncing": { "type": "boolean" }
                }
            },
            "ReadyResponse": {
                "type": "object",
                "properties": {
                    "ready": { "type": "boolean" },
                    "reason": { "type": "string" }
                }
            },
            "Coin": {
                "type": "object",
                "properties": {
                    "denom": { "type": "string" },
                    "amount": { "type": "string" }
                }
            },
            "QueryAllBalancesResponse": {
                "type": "object",
                "properties": {
                    "balances": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Coin" }
                    },
                    "pagination": { "$ref": "#/definitions/PageResponse" }
                }
            },
            "QueryBalanceResponse": {
                "type": "object",
                "properties": {
                    "balance": { "$ref": "#/definitions/Coin" }
                }
            },
            "QueryTotalSupplyResponse": {
                "type": "object",
                "properties": {
                    "supply": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Coin" }
                    },
                    "pagination": { "$ref": "#/definitions/PageResponse" }
                }
            },
            "QueryAccountResponse": {
                "type": "object",
                "properties": {
                    "account": { "$ref": "#/definitions/Any" }
                }
            },
            "Any": {
                "type": "object",
                "properties": {
                    "type_url": { "type": "string" },
                    "value": { "type": "string", "format": "byte" }
                }
            },
            "PageRequest": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "format": "byte" },
                    "offset": { "type": "string", "format": "uint64" },
                    "limit": { "type": "string", "format": "uint64" },
                    "count_total": { "type": "boolean" },
                    "reverse": { "type": "boolean" }
                }
            },
            "PageResponse": {
                "type": "object",
                "properties": {
                    "next_key": { "type": "string", "format": "byte" },
                    "total": { "type": "string", "format": "uint64" }
                }
            },
            "SimulateRequest": {
                "type": "object",
                "properties": {
                    "tx": { "$ref": "#/definitions/Tx" },
                    "tx_bytes": { "type": "string", "format": "byte" }
                }
            },
            "SimulateResponse": {
                "type": "object",
                "properties": {
                    "gas_info": { "$ref": "#/definitions/GasInfo" },
                    "result": { "$ref": "#/definitions/Result" }
                }
            },
            "BroadcastTxRequest": {
                "type": "object",
                "properties": {
                    "tx_bytes": { "type": "string", "format": "byte" },
                    "mode": { "type": "string", "enum": ["BROADCAST_MODE_UNSPECIFIED", "BROADCAST_MODE_BLOCK", "BROADCAST_MODE_SYNC", "BROADCAST_MODE_ASYNC"] }
                }
            },
            "BroadcastTxResponse": {
                "type": "object",
                "properties": {
                    "tx_response": { "$ref": "#/definitions/TxResponse" }
                }
            },
            "Tx": {
                "type": "object",
                "properties": {
                    "body": { "$ref": "#/definitions/TxBody" },
                    "auth_info": { "$ref": "#/definitions/AuthInfo" },
                    "signatures": {
                        "type": "array",
                        "items": { "type": "string", "format": "byte" }
                    }
                }
            },
            "TxBody": {
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Any" }
                    },
                    "memo": { "type": "string" },
                    "timeout_height": { "type": "string", "format": "uint64" },
                    "extension_options": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Any" }
                    },
                    "non_critical_extension_options": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Any" }
                    }
                }
            },
            "AuthInfo": {
                "type": "object",
                "properties": {
                    "signer_infos": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/SignerInfo" }
                    },
                    "fee": { "$ref": "#/definitions/Fee" }
                }
            },
            "SignerInfo": {
                "type": "object",
                "properties": {
                    "public_key": { "$ref": "#/definitions/Any" },
                    "mode_info": { "$ref": "#/definitions/ModeInfo" },
                    "sequence": { "type": "string", "format": "uint64" }
                }
            },
            "ModeInfo": {
                "type": "object",
                "properties": {
                    "single": { "$ref": "#/definitions/ModeInfoSingle" },
                    "multi": { "$ref": "#/definitions/ModeInfoMulti" }
                }
            },
            "ModeInfoSingle": {
                "type": "object",
                "properties": {
                    "mode": { "type": "string" }
                }
            },
            "ModeInfoMulti": {
                "type": "object",
                "properties": {
                    "bitarray": { "$ref": "#/definitions/CompactBitArray" },
                    "mode_infos": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/ModeInfo" }
                    }
                }
            },
            "CompactBitArray": {
                "type": "object",
                "properties": {
                    "extra_bits_stored": { "type": "integer", "format": "uint32" },
                    "elems": { "type": "string", "format": "byte" }
                }
            },
            "Fee": {
                "type": "object",
                "properties": {
                    "amount": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Coin" }
                    },
                    "gas_limit": { "type": "string", "format": "uint64" },
                    "payer": { "type": "string" },
                    "granter": { "type": "string" }
                }
            },
            "GasInfo": {
                "type": "object",
                "properties": {
                    "gas_wanted": { "type": "string", "format": "uint64" },
                    "gas_used": { "type": "string", "format": "uint64" }
                }
            },
            "Result": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "format": "byte" },
                    "log": { "type": "string" },
                    "events": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Event" }
                    },
                    "msg_responses": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Any" }
                    }
                }
            },
            "Event": {
                "type": "object",
                "properties": {
                    "type": { "type": "string" },
                    "attributes": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/EventAttribute" }
                    }
                }
            },
            "EventAttribute": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "value": { "type": "string" },
                    "index": { "type": "boolean" }
                }
            },
            "TxResponse": {
                "type": "object",
                "properties": {
                    "height": { "type": "string", "format": "int64" },
                    "txhash": { "type": "string" },
                    "codespace": { "type": "string" },
                    "code": { "type": "integer", "format": "uint32" },
                    "data": { "type": "string" },
                    "raw_log": { "type": "string" },
                    "logs": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/ABCIMessageLog" }
                    },
                    "info": { "type": "string" },
                    "gas_wanted": { "type": "string", "format": "int64" },
                    "gas_used": { "type": "string", "format": "int64" },
                    "tx": { "$ref": "#/definitions/Any" },
                    "timestamp": { "type": "string" },
                    "events": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Event" }
                    }
                }
            },
            "ABCIMessageLog": {
                "type": "object",
                "properties": {
                    "msg_index": { "type": "integer", "format": "uint32" },
                    "log": { "type": "string" },
                    "events": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/StringEvent" }
                    }
                }
            },
            "StringEvent": {
                "type": "object",
                "properties": {
                    "type": { "type": "string" },
                    "attributes": {
                        "type": "array",
                        "items": { "$ref": "#/definitions/Attribute" }
                    }
                }
            },
            "Attribute": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                }
            }
        }),
    }
}

/// Swagger JSON handler
pub async fn swagger_json_handler() -> Result<Json<SwaggerSpec>, StatusCode> {
    let spec = generate_swagger_spec();
    Ok(Json(spec))
}

/// Swagger UI HTML handler
pub async fn swagger_ui_handler() -> Result<Html<String>, StatusCode> {
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Helium API - Swagger UI</title>
    <link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
    <style>
        html {
            box-sizing: border-box;
            overflow: -moz-scrollbars-vertical;
            overflow-y: scroll;
        }
        *, *:before, *:after {
            box-sizing: inherit;
        }
        body {
            margin:0;
            background: #fafafa;
        }
    </style>
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
    <script>
    window.onload = function() {
        window.ui = SwaggerUIBundle({
            url: "/swagger.json",
            dom_id: '#swagger-ui',
            deepLinking: true,
            presets: [
                SwaggerUIBundle.presets.apis,
                SwaggerUIStandalonePreset
            ],
            plugins: [
                SwaggerUIBundle.plugins.DownloadUrl
            ],
            layout: "StandaloneLayout"
        });
    };
    </script>
</body>
</html>"#;

    Ok(Html(html.to_string()))
}

/// Create swagger router
pub fn swagger_router() -> Router {
    Router::new()
        .route("/swagger", get(swagger_ui_handler))
        .route("/swagger.json", get(swagger_json_handler))
}