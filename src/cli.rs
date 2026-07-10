use crate::{
    config::{GulfConfig, ProviderSummary},
    model::{ExecuteSwapResult, PartnerFeeRequest, PrepareSwapRequest, QuoteRequest},
    provider::ProviderRegistry,
    server,
};
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "gulf",
    version,
    about = "Universal bridge/swap CLI and API server"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        env = "GULF_CONFIG",
        default_value = "config/gulf.toml"
    )]
    config: String,
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "List configured bridge and swap providers sorted by estimated fee")]
    Providers(ProvidersArgs),
    #[command(about = "List configured blockchains")]
    Chains(OutputArgs),
    #[command(about = "List configured tokens")]
    Tokens(OutputArgs),
    #[command(about = "Request route quotes from enabled providers")]
    Quote(QuoteArgs),
    #[command(about = "Prepare a swap/bridge transaction payload")]
    Swap {
        #[command(subcommand)]
        command: SwapCommand,
    },
    #[command(about = "Run the Gulf HTTP API")]
    Serve(ServeArgs),
    #[command(about = "Validate the configuration file")]
    Check,
}

#[derive(Args, Debug)]
struct ProvidersArgs {}
#[derive(Args, Debug)]
struct OutputArgs {}
#[derive(Args, Debug)]
struct QuoteArgs {
    #[arg(long)]
    from_chain: String,
    #[arg(long)]
    to_chain: String,
    #[arg(long)]
    from_token: String,
    #[arg(long)]
    to_token: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    sender: String,
    #[arg(long)]
    recipient: Option<String>,
    #[arg(long, default_value_t = false)]
    live: bool,
    #[arg(long)]
    partner_id: Option<String>,
    #[arg(long)]
    partner_fee_bps: Option<f64>,
    #[arg(long)]
    partner_recipient: Option<String>,
}
#[derive(Subcommand, Debug)]
enum SwapCommand {
    #[command(about = "Build an unsigned transaction or provider order")]
    Prepare(PrepareArgs),
    #[command(about = "Locally sign and submit supported EVM swap actions")]
    Execute(ExecuteArgs),
}
#[derive(Args, Debug)]
struct PrepareArgs {
    #[command(flatten)]
    quote: QuoteArgs,
    #[arg(long)]
    provider_id: Option<String>,
}
#[derive(Args, Debug)]
struct ExecuteArgs {
    #[command(flatten)]
    quote: QuoteArgs,
    #[arg(long)]
    provider_id: Option<String>,
    #[arg(long, default_value = "GULF_EVM_PRIVATE_KEY")]
    private_key_env: String,
    #[arg(long, required = true)]
    yes: bool,
}
#[derive(Args, Debug)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
}

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let json_output = cli.json;
    let config = GulfConfig::load(&cli.config).await?;
    let registry = ProviderRegistry::new(config)?;
    match cli.command {
        Command::Providers(_) => {
            let providers = registry.sorted_providers();
            if json_output {
                print_json(
                    "providers",
                    providers.iter().map(ProviderSummary::from).collect(),
                )?;
            } else {
                for provider in providers {
                    println!(
                        "{:<24} {:>8.3} bps  {}",
                        provider.id,
                        provider.fee.estimated_bps.unwrap_or_default(),
                        provider.name
                    );
                }
            }
        }
        Command::Chains(_) => {
            let chains = registry.chains();
            if json_output {
                print_json("chains", chains)?;
            } else {
                for chain in chains {
                    println!(
                        "{:<18} {:<12} {}",
                        chain.caip2_id,
                        chain.vm,
                        chain
                            .numeric_chain_id
                            .map(|id| id.to_string())
                            .unwrap_or_default()
                    );
                }
            }
        }
        Command::Tokens(_) => {
            let tokens = registry.tokens();
            if json_output {
                print_json("tokens", tokens)?;
            } else {
                for token in tokens {
                    println!(
                        "{:<24} {:<10} {}",
                        token.id,
                        token.symbol,
                        token.address.unwrap_or_else(|| "native".to_owned())
                    );
                }
            }
        }
        Command::Quote(args) => println!(
            "{}",
            serde_json::to_string_pretty(&registry.quote_all(&quote_request(args), false).await?)?
        ),
        Command::Swap {
            command: SwapCommand::Prepare(args),
        } => {
            let prepared = registry
                .prepare_swap(&PrepareSwapRequest {
                    quote: quote_request(args.quote),
                    provider_id: args.provider_id,
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&prepared)?);
        }
        Command::Swap {
            command: SwapCommand::Execute(args),
        } => {
            if !args.yes {
                anyhow::bail!("--yes is required to submit transactions");
            }
            let prepared = registry
                .prepare_swap(&PrepareSwapRequest {
                    quote: quote_request(args.quote),
                    provider_id: args.provider_id,
                })
                .await?;
            let result =
                crate::execution::execute(registry.config(), prepared, &args.private_key_env)
                    .await?;
            print_execution_result(&result, json_output)?;
        }
        Command::Serve(args) => server::serve(args.listen, registry).await?,
        Command::Check => println!(
            "ok: loaded {} enabled providers from {}",
            registry.sorted_providers().len(),
            display_config_source(&cli.config)
        ),
    }
    Ok(())
}

fn print_execution_result(result: &ExecuteSwapResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
    } else {
        for receipt in &result.receipts {
            println!(
                "{} {} block {} {} {}",
                receipt.transaction_hash,
                receipt.chain,
                receipt.block_number,
                receipt.status,
                receipt.action_type
            );
        }
    }
    Ok(())
}

fn print_json<T: serde::Serialize>(name: &str, values: Vec<T>) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ name: values }))?
    );
    Ok(())
}

fn quote_request(args: QuoteArgs) -> QuoteRequest {
    QuoteRequest {
        from_chain: args.from_chain,
        to_chain: args.to_chain,
        from_token: args.from_token,
        to_token: args.to_token,
        amount: args.amount,
        sender: args.sender,
        recipient: args.recipient,
        live: args.live,
        partner_fee: args.partner_id.map(|partner_id| PartnerFeeRequest {
            partner_id,
            fee_bps: args.partner_fee_bps,
            recipient: args.partner_recipient,
            metadata: Default::default(),
        }),
    }
}

fn display_config_source(config: &str) -> String {
    PathBuf::from(config)
        .canonicalize()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| config.to_owned())
}
