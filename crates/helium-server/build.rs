use std::io::Result;

fn main() -> Result<()> {
    // Compile proto files
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile(
            &[
                "proto/cosmos/bank/v1beta1/query.proto",
                "proto/cosmos/bank/v1beta1/bank.proto",
                "proto/cosmos/auth/v1beta1/query.proto",
                "proto/cosmos/auth/v1beta1/auth.proto",
                "proto/cosmos/tx/v1beta1/service.proto",
                "proto/cosmos/tx/v1beta1/tx.proto",
                "proto/cosmos/base/query/v1beta1/pagination.proto",
                "proto/cosmos/base/abci/v1beta1/abci.proto",
                "proto/cometbft/abci/v1/types.proto",
            ],
            &["proto"],
        )?;

    Ok(())
}
