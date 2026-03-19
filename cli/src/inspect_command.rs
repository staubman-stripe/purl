//! Inspect command for viewing payment requirements without executing payment

use anyhow::{Context, Result};
use colored::Colorize;
use purl_lib::protocol::PaymentChallenge;
use purl_lib::{
    HttpClientBuilder, HttpMethod, PaymentMethod, PaymentRequirementsResponse, PROTOCOL_REGISTRY,
};
use serde::Serialize;
use std::borrow::Cow;

use crate::cli::{Cli, OutputFormat};
use crate::config_utils::load_config;
use crate::hyperlink::address_link;

/// Output format for a single payment option
#[derive(Debug, Serialize)]
struct AcceptedPaymentOption {
    network: String,
    scheme: String,
    amount_atomic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount_human: Option<String>,
    asset: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    decimals: Option<u8>,
    compatible: bool,
    pay_to: String,
    description: String,
}

/// Output format for the inspect command
#[derive(Debug, Serialize)]
struct InspectOutput {
    x402_version: u32,
    message: String,
    accepts: Vec<AcceptedPaymentOption>,
    configured_methods: Vec<String>,
}

/// Format atomic units to human-readable amounts
fn format_amount(atomic: &str, decimals: u8, symbol: &str) -> String {
    use purl_lib::currency::format_atomic_trimmed;
    format_atomic_trimmed(atomic, decimals, symbol)
}

/// Get token symbol and whether it came from seller-provided `extra["symbol"]` field
fn get_token_symbol(requirement: &purl_lib::x402::PaymentRequirements) -> (String, bool) {
    purl_lib::constants::get_token_symbol(requirement.network(), requirement.asset())
        .map(|sym| (sym.to_string(), false))
        .or_else(|| {
            requirement
                .extra()
                .and_then(|e| e.get("symbol").and_then(|s| s.as_str()))
                .map(|sym| (sym.to_string(), true))
        })
        .unwrap_or_else(|| (requirement.asset().to_string(), false))
}

/// Get decimals for a token on a network
fn get_decimals(network: &str, asset: &str) -> Result<u8> {
    purl_lib::constants::get_token_decimals(network, asset).map_err(|e| {
        anyhow::anyhow!("Token decimals not configured: {e}. Add to ~/.purl/tokens.json if needed.")
    })
}

/// Inspect payment requirements for a URL
pub async fn inspect_command(cli: &Cli, url: &str) -> Result<()> {
    let config = load_config(cli.config.as_ref())?;

    // Determine HTTP method and body from CLI flags
    let body = cli
        .json
        .as_ref()
        .or(cli.data.as_ref())
        .map(|s| s.as_bytes().to_vec());

    let method = cli
        .method
        .as_ref()
        .map(|s| HttpMethod::from_bytes(s.as_bytes()).unwrap_or(HttpMethod::GET))
        .unwrap_or_else(|| {
            if body.is_some() {
                HttpMethod::POST
            } else {
                HttpMethod::GET
            }
        });

    // Build HTTP client with headers
    let mut headers = cli
        .parse_headers()
        .map_err(|e| anyhow::anyhow!("Invalid header: {}", e))?;

    // Auto-set Content-Type for JSON data
    if cli.json.is_some()
        || cli
            .data
            .as_ref()
            .map(|d| d.trim().starts_with('{') || d.trim().starts_with('['))
            .unwrap_or(false)
    {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }

    let mut builder = HttpClientBuilder::new()
        .verbose(cli.is_verbose())
        .follow_redirects(cli.follow_redirects)
        .headers(&headers);

    if let Some(timeout) = cli.get_timeout() {
        builder = builder.timeout(timeout);
    }

    if let Some(user_agent) = &cli.user_agent {
        builder = builder.user_agent(user_agent);
    }

    let client = builder.build()?;

    if cli.is_verbose() && cli.should_show_output() {
        eprintln!("Inspecting payment requirements for: {url} ({})", method);
    }

    let response = client.request(method, url, body.as_deref()).await?;

    if !response.is_payment_required() {
        anyhow::bail!(
            "No payment required (status: {}). URL does not require payment.",
            response.status_code
        );
    }

    let available_methods = config.available_payment_methods();
    let format = cli.output_format.resolve();

    // Collect challenges from all matching protocols
    let handlers = PROTOCOL_REGISTRY.find_all_handlers(&response);
    let mut challenges: Vec<Box<dyn PaymentChallenge>> = Vec::new();
    for handler in &handlers {
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Detected protocol: {}", handler.name());
        }
        match handler.parse_challenges(&response) {
            Ok(parsed) => challenges.extend(parsed),
            Err(e) => {
                if cli.is_verbose() && cli.should_show_output() {
                    eprintln!(
                        "Warning: failed to parse {} challenges: {}",
                        handler.name(),
                        e
                    );
                }
            }
        }
    }

    if challenges.is_empty() {
        // Fall back to raw x402 parsing for legacy display
        let json = response.payment_requirements_json()?;
        let requirements: PaymentRequirementsResponse =
            serde_json::from_str(&json).context("Failed to parse payment requirements")?;

        match format {
            OutputFormat::Auto => unreachable!("Auto should be resolved"),
            OutputFormat::Json => output_json(cli, &requirements, &available_methods)?,
            OutputFormat::Yaml => output_yaml(cli, &requirements, &available_methods)?,
            OutputFormat::Text => output_text(cli, &requirements, &available_methods)?,
        }
    } else {
        let protocol_names: Vec<&str> = handlers.iter().map(|h| h.name()).collect();

        match format {
            OutputFormat::Auto => unreachable!("Auto should be resolved"),
            OutputFormat::Json => {
                output_mpp_json(cli, &protocol_names, &challenges, &available_methods)?
            }
            OutputFormat::Yaml => {
                output_mpp_yaml(cli, &protocol_names, &challenges, &available_methods)?
            }
            OutputFormat::Text => {
                output_mpp_text(cli, &protocol_names, &challenges, &available_methods)?
            }
        }
    }

    Ok(())
}

/// Build structured output from payment requirements
fn build_inspect_output(
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> InspectOutput {
    let accepts = requirements
        .accepts()
        .iter()
        .map(|req| {
            let (symbol, seller_provided) = get_token_symbol(req);
            let decimals = get_decimals(req.network(), req.asset()).ok();
            let amount_result = req.parse_max_amount();
            let amount_atomic = amount_result
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_else(|_| "invalid".to_string());
            let amount_human = decimals.zip(amount_result.ok()).map(|(dec, amt)| {
                format!(
                    "{} ({})",
                    format_amount(
                        &amt.to_string(),
                        dec,
                        &(if seller_provided {
                            Cow::Owned(format!("\"{}\"", symbol))
                        } else {
                            Cow::Borrowed(&symbol)
                        }),
                    ),
                    purl_lib::network::resolve_network_alias(req.network())
                )
            });

            AcceptedPaymentOption {
                network: req.network().to_string(),
                scheme: req.scheme().to_string(),
                amount_atomic,
                amount_human,
                asset: req.asset().to_string(),
                symbol,
                decimals,
                compatible: is_compatible_method(req, available_methods),
                pay_to: req.pay_to().to_string(),
                description: req.description().to_string(),
            }
        })
        .collect();

    InspectOutput {
        x402_version: requirements.version(),
        message: requirements
            .error()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        accepts,
        configured_methods: available_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect(),
    }
}

fn output_json(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(requirements, available_methods);
    let pretty_json = serde_json::to_string_pretty(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &pretty_json)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{pretty_json}");
    }

    Ok(())
}

fn output_yaml(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_inspect_output(requirements, available_methods);
    let yaml_output = serde_yaml::to_string(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &yaml_output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{yaml_output}");
    }

    Ok(())
}

fn output_text(
    cli: &Cli,
    requirements: &PaymentRequirementsResponse,
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let data = build_inspect_output(requirements, available_methods);

    // When writing to file, use plain text (no ANSI codes)
    if let Some(output_file) = &cli.output {
        let mut output = String::new();
        output.push_str("Payment Required (402)\n");
        output.push_str(&format!("Message: {}\n", data.message));
        output.push_str(&format!("x402 Version: {}\n", data.x402_version));
        output.push_str("\nAccepts:\n");

        for opt in &data.accepts {
            output.push_str(&format!("  - Network: {}\n", opt.network));
            output.push_str(&format!("    Scheme: {}\n", opt.scheme));
            if let Some(ref human) = opt.amount_human {
                output.push_str(&format!(
                    "    Amount: {} ({} atomic units)\n",
                    human, opt.amount_atomic
                ));
            } else {
                output.push_str(&format!(
                    "    Amount: {} atomic units (decimals unknown)\n",
                    opt.amount_atomic
                ));
            }
            output.push_str(&format!("    Asset: {}\n", opt.asset));
            output.push_str(&format!("    Pay To: {}\n", opt.pay_to));
            if !opt.description.is_empty() {
                output.push_str(&format!("    Description: {}\n", opt.description));
            }
            if opt.compatible {
                output.push_str("    Compatible: Yes (configured)\n");
            } else {
                output.push_str("    Compatible: No (not configured)\n");
            }
            output.push('\n');
        }

        output.push_str(&format!(
            "Configured payment methods: {}\n",
            if data.configured_methods.is_empty() {
                "None".to_string()
            } else {
                data.configured_methods.join(", ")
            }
        ));

        std::fs::write(output_file, &output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
        return Ok(());
    }

    // Colored output for terminal
    println!("{}", "Payment Required (402)".yellow().bold());

    if !data.message.is_empty() {
        println!("{}: {}", "Message".bold(), data.message);
    }
    println!("{}: {}", "x402 Version".bold(), data.x402_version);
    println!();
    println!("{}", "Accepts:".green().bold());

    for opt in &data.accepts {
        println!("  {} {}", "•".cyan(), opt.network.cyan());
        println!("    {}: {}", "Scheme".dimmed(), opt.scheme);

        if let Some(ref human) = opt.amount_human {
            println!(
                "    {}: {} {}",
                "Amount".dimmed(),
                human.yellow(),
                format!("({} atomic)", opt.amount_atomic).dimmed()
            );
        } else {
            println!(
                "    {}: {} {}",
                "Amount".dimmed(),
                opt.amount_atomic.yellow(),
                "atomic (decimals unknown)".dimmed()
            );
        }

        // Link asset address to explorer
        let asset_link = address_link(&opt.asset, &opt.network);
        println!(
            "    {}: {} {}",
            "Asset".dimmed(),
            asset_link.yellow(),
            format!("({})", opt.symbol).dimmed()
        );

        // Link pay_to address to explorer
        let pay_to_link = address_link(&opt.pay_to, &opt.network);
        println!("    {}: {}", "Pay To".dimmed(), pay_to_link.yellow());

        if !opt.description.is_empty() {
            println!("    {}: {}", "Description".dimmed(), opt.description);
        }

        if opt.compatible {
            println!(
                "    {}: {}",
                "Compatible".dimmed(),
                "Yes (configured)".green()
            );
        } else {
            println!(
                "    {}: {}",
                "Compatible".dimmed(),
                "No (not configured)".red()
            );
        }

        println!();
    }

    let methods_str = if data.configured_methods.is_empty() {
        "None".red().to_string()
    } else {
        data.configured_methods.join(", ")
    };
    println!("{}: {}", "Configured payment methods".bold(), methods_str);

    Ok(())
}

/// Output format for multi-protocol challenges
#[derive(Debug, Serialize)]
struct MppInspectOutput {
    protocols: Vec<String>,
    challenges: Vec<MppChallengeOutput>,
    configured_methods: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MppChallengeOutput {
    protocol: String,
    scheme: String,
    network: String,
    amount_atomic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount_human: Option<String>,
    asset: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    decimals: Option<u8>,
    recipient: String,
    description: String,
    resource: String,
    compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<String>,
}

fn build_mpp_output(
    protocol_names: &[&str],
    challenges: &[Box<dyn PaymentChallenge>],
    available_methods: &[PaymentMethod],
) -> MppInspectOutput {
    let challenge_outputs = challenges
        .iter()
        .map(|challenge| {
            let network = challenge.network();
            let asset = challenge.asset();
            let decimals = purl_lib::constants::get_token_decimals(network, asset).ok();
            let symbol = purl_lib::constants::get_token_symbol(network, asset)
                .map(|s| s.to_string())
                .unwrap_or_else(|| asset.to_string());

            let amount_human = decimals.map(|dec| {
                format!(
                    "{} ({})",
                    format_amount(challenge.amount(), dec, &symbol),
                    purl_lib::network::resolve_network_alias(network)
                )
            });

            // Get MPP-specific fields if available
            let (challenge_id, method) = if let Some(mpp_challenge) = challenge
                .as_any()
                .downcast_ref::<purl_lib::mpp::MppChallenge>(
            ) {
                (
                    Some(mpp_challenge.inner.id.clone()),
                    Some(mpp_challenge.inner.method.as_str().to_string()),
                )
            } else {
                (None, None)
            };

            MppChallengeOutput {
                protocol: challenge.protocol_name().to_string(),
                scheme: challenge.scheme().to_string(),
                network: network.to_string(),
                amount_atomic: challenge.amount().to_string(),
                amount_human,
                asset: asset.to_string(),
                symbol,
                decimals,
                recipient: challenge.recipient().to_string(),
                description: challenge.description().to_string(),
                resource: challenge.resource().to_string(),
                compatible: is_challenge_compatible(challenge.as_ref(), available_methods),
                challenge_id,
                method,
            }
        })
        .collect();

    MppInspectOutput {
        protocols: protocol_names.iter().map(|s| s.to_string()).collect(),
        challenges: challenge_outputs,
        configured_methods: available_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect(),
    }
}

fn output_mpp_json(
    cli: &Cli,
    protocol_names: &[&str],
    challenges: &[Box<dyn PaymentChallenge>],
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_mpp_output(protocol_names, challenges, available_methods);
    let pretty_json = serde_json::to_string_pretty(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &pretty_json)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{pretty_json}");
    }

    Ok(())
}

fn output_mpp_yaml(
    cli: &Cli,
    protocol_names: &[&str],
    challenges: &[Box<dyn PaymentChallenge>],
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let output = build_mpp_output(protocol_names, challenges, available_methods);
    let yaml_output = serde_yaml::to_string(&output)?;

    if let Some(output_file) = &cli.output {
        std::fs::write(output_file, &yaml_output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{yaml_output}");
    }

    Ok(())
}

fn output_mpp_text(
    cli: &Cli,
    protocol_names: &[&str],
    challenges: &[Box<dyn PaymentChallenge>],
    available_methods: &[PaymentMethod],
) -> Result<()> {
    let data = build_mpp_output(protocol_names, challenges, available_methods);

    // When writing to file, use plain text (no ANSI codes)
    if let Some(output_file) = &cli.output {
        let mut output = String::new();
        output.push_str("Payment Required (402)\n");
        output.push_str(&format!("Protocols: {}\n", data.protocols.join(", ")));
        output.push_str(&format!("Payment Options: {}\n", data.challenges.len()));
        output.push_str("\nChallenges:\n");

        for (i, opt) in data.challenges.iter().enumerate() {
            output.push_str(&format!("\n  Option {}:\n", i + 1));
            output.push_str(&format!("    Protocol: {}\n", opt.protocol));
            output.push_str(&format!("    Scheme: {}\n", opt.scheme));
            output.push_str(&format!("    Network: {}\n", opt.network));
            if let Some(ref human) = opt.amount_human {
                output.push_str(&format!(
                    "    Amount: {} ({} atomic)\n",
                    human, opt.amount_atomic
                ));
            } else {
                output.push_str(&format!(
                    "    Amount: {} atomic (decimals unknown)\n",
                    opt.amount_atomic
                ));
            }
            output.push_str(&format!("    Asset: {} ({})\n", opt.symbol, opt.asset));
            output.push_str(&format!("    Recipient: {}\n", opt.recipient));
            if !opt.description.is_empty() {
                output.push_str(&format!("    Description: {}\n", opt.description));
            }
            if !opt.resource.is_empty() {
                output.push_str(&format!("    Resource: {}\n", opt.resource));
            }
            if let Some(ref id) = opt.challenge_id {
                output.push_str(&format!("    Challenge ID: {}\n", id));
            }
            if let Some(ref method) = opt.method {
                output.push_str(&format!("    Method: {}\n", method));
            }
            if opt.compatible {
                output.push_str("    Compatible: Yes (configured)\n");
            } else {
                output.push_str("    Compatible: No (not configured)\n");
            }
        }

        output.push_str(&format!(
            "\nConfigured payment methods: {}\n",
            if data.configured_methods.is_empty() {
                "None".to_string()
            } else {
                data.configured_methods.join(", ")
            }
        ));

        std::fs::write(output_file, &output)?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
        return Ok(());
    }

    // Colored output for terminal
    println!("{}", "Payment Required (402)".yellow().bold());
    println!(
        "{}: {}",
        "Protocols".bold(),
        data.protocols.join(", ").cyan()
    );
    println!(
        "{}: {}",
        "Payment Options".bold(),
        data.challenges.len().to_string().cyan()
    );
    println!();
    println!("{}", "Challenges:".green().bold());

    for (i, opt) in data.challenges.iter().enumerate() {
        println!();
        println!(
            "  {} {}",
            format!("Option {}:", i + 1).cyan().bold(),
            opt.network.cyan()
        );
        println!("    {}: {}", "Protocol".dimmed(), opt.protocol);
        println!("    {}: {}", "Scheme".dimmed(), opt.scheme);

        if let Some(ref human) = opt.amount_human {
            println!(
                "    {}: {} {}",
                "Amount".dimmed(),
                human.yellow(),
                format!("({} atomic)", opt.amount_atomic).dimmed()
            );
        } else {
            println!(
                "    {}: {} {}",
                "Amount".dimmed(),
                opt.amount_atomic.yellow(),
                "atomic (decimals unknown)".dimmed()
            );
        }

        // Link asset address to explorer
        let asset_link = address_link(&opt.asset, &opt.network);
        println!(
            "    {}: {} {}",
            "Asset".dimmed(),
            opt.symbol.yellow(),
            format!("({})", asset_link).dimmed()
        );

        // Link recipient address to explorer
        let recipient_link = address_link(&opt.recipient, &opt.network);
        println!("    {}: {}", "Recipient".dimmed(), recipient_link.yellow());

        if !opt.description.is_empty() {
            println!("    {}: {}", "Description".dimmed(), opt.description);
        }
        if !opt.resource.is_empty() {
            println!("    {}: {}", "Resource".dimmed(), opt.resource);
        }
        if let Some(ref id) = opt.challenge_id {
            println!("    {}: {}", "Challenge ID".dimmed(), id);
        }
        if let Some(ref method) = opt.method {
            println!("    {}: {}", "Method".dimmed(), method);
        }

        if opt.compatible {
            println!(
                "    {}: {}",
                "Compatible".dimmed(),
                "Yes (configured)".green()
            );
        } else {
            println!(
                "    {}: {}",
                "Compatible".dimmed(),
                "No (not configured)".red()
            );
        }
    }

    println!();
    let methods_str = if data.configured_methods.is_empty() {
        "None".red().to_string()
    } else {
        data.configured_methods.join(", ")
    };
    println!("{}: {}", "Configured payment methods".bold(), methods_str);

    Ok(())
}

/// Check if a challenge is compatible with configured payment methods
fn is_challenge_compatible(
    challenge: &dyn PaymentChallenge,
    available_methods: &[PaymentMethod],
) -> bool {
    // Tempo challenges work with both Tempo and EVM wallets (EVM-compatible)
    if challenge.is_tempo()
        && (available_methods.contains(&PaymentMethod::Tempo)
            || available_methods.contains(&PaymentMethod::Evm))
    {
        return true;
    }
    // EVM challenges work with EVM wallets (but not Tempo-only wallets for non-Tempo chains)
    if challenge.is_evm() && available_methods.contains(&PaymentMethod::Evm) {
        return true;
    }
    if challenge.is_solana() && available_methods.contains(&PaymentMethod::Solana) {
        return true;
    }
    false
}

/// Check if a requirement is compatible with configured payment methods
fn is_compatible_method(
    req: &purl_lib::x402::PaymentRequirements,
    available_methods: &[PaymentMethod],
) -> bool {
    // Tempo challenges work with both Tempo and EVM wallets (EVM-compatible)
    if req.is_tempo()
        && (available_methods.contains(&PaymentMethod::Tempo)
            || available_methods.contains(&PaymentMethod::Evm))
    {
        return true;
    }
    // EVM challenges work with EVM wallets
    if req.is_evm() && available_methods.contains(&PaymentMethod::Evm) {
        return true;
    }
    if req.is_solana() && available_methods.contains(&PaymentMethod::Solana) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_amount_eth() {
        // 1 ETH = 1000000000000000000 wei
        assert_eq!(format_amount("1000000000000000000", 18, "ETH"), "1 ETH");
        assert_eq!(format_amount("100000000000000000", 18, "ETH"), "0.1 ETH");
        assert_eq!(format_amount("10000000000000000", 18, "ETH"), "0.01 ETH");
        assert_eq!(format_amount("1000000000000000", 18, "ETH"), "0.001 ETH");
    }

    #[test]
    fn test_format_amount_usdc() {
        // USDC has 6 decimals
        assert_eq!(format_amount("1000000", 6, "USDC"), "1 USDC");
        assert_eq!(format_amount("100000", 6, "USDC"), "0.1 USDC");
        assert_eq!(format_amount("10000", 6, "USDC"), "0.01 USDC");
        assert_eq!(format_amount("1000", 6, "USDC"), "0.001 USDC");
    }

    #[test]
    fn test_format_amount_sol() {
        // SOL has 9 decimals (lamports)
        assert_eq!(format_amount("1000000000", 9, "SOL"), "1 SOL");
        assert_eq!(format_amount("100000000", 9, "SOL"), "0.1 SOL");
        assert_eq!(format_amount("10000000", 9, "SOL"), "0.01 SOL");
        assert_eq!(format_amount("1000000", 9, "SOL"), "0.001 SOL");
    }

    #[test]
    fn test_format_amount_no_fraction() {
        assert_eq!(format_amount("5000000000000000000", 18, "ETH"), "5 ETH");
        assert_eq!(format_amount("5000000", 6, "USDC"), "5 USDC");
    }

    #[test]
    fn test_format_amount_zero() {
        assert_eq!(format_amount("0", 18, "ETH"), "0 ETH");
        assert_eq!(format_amount("0", 6, "USDC"), "0 USDC");
    }
}

#[cfg(test)]
mod get_token_symbol_tests;
