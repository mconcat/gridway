use std::io::Result;

fn main() -> Result<()> {
    // Configure protobuf code generation
    let config = tonic_build::configure();

    // Compile all proto files
    config.compile_protos(
        &[
            // CometBFT ABCI types
            "proto/cometbft/abci/v1/types.proto",
            // Cosmos SDK types
            "proto/cosmos/base/abci/v1beta1/abci.proto",
            "proto/cosmos/base/query/v1beta1/pagination.proto",
            "proto/cosmos/tx/v1beta1/tx.proto",
            "proto/cosmos/tx/v1beta1/service.proto",
            "proto/cosmos/auth/v1beta1/auth.proto",
            "proto/cosmos/auth/v1beta1/query.proto",
            "proto/cosmos/bank/v1beta1/bank.proto",
            "proto/cosmos/bank/v1beta1/query.proto",
            // Google protobuf types
            "proto/google/protobuf/duration.proto",
            "proto/google/protobuf/timestamp.proto",
        ],
        &["proto"],
    )?;

    Ok(())
}
