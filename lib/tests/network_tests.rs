//! Integration tests for network and token configuration

use purl_lib::constants::{get_token_decimals, get_token_symbol};
use purl_lib::network::{
    get_evm_chain_id, get_network, is_evm_network, is_solana_network, is_tempo_network,
    resolve_network_alias, ChainType, Network,
};

#[test]
fn test_network_enum_has_entries() {
    assert!(!Network::all().is_empty());
}

#[test]
fn test_get_network() {
    struct TestCase {
        name: &'static str,
        expected_chain_type: ChainType,
        expected_chain_id: Option<u64>,
        expected_mainnet: bool,
        expected_display_name: Option<&'static str>,
    }

    let test_cases = vec![
        TestCase {
            name: "ethereum",
            expected_chain_type: ChainType::Evm,
            expected_chain_id: Some(1),
            expected_mainnet: true,
            expected_display_name: Some("Ethereum"),
        },
        TestCase {
            name: "base",
            expected_chain_type: ChainType::Evm,
            expected_chain_id: Some(8453),
            expected_mainnet: true,
            expected_display_name: None,
        },
        TestCase {
            name: "base-sepolia",
            expected_chain_type: ChainType::Evm,
            expected_chain_id: Some(84532),
            expected_mainnet: false,
            expected_display_name: None,
        },
        TestCase {
            name: "solana",
            expected_chain_type: ChainType::Solana,
            expected_chain_id: None,
            expected_mainnet: true,
            expected_display_name: None,
        },
        TestCase {
            name: "solana-devnet",
            expected_chain_type: ChainType::Solana,
            expected_chain_id: None,
            expected_mainnet: false,
            expected_display_name: None,
        },
    ];

    for test_case in test_cases {
        let network = get_network(test_case.name);
        assert!(network.is_some(), "Network {} should exist", test_case.name);

        let info = network.unwrap();
        // Note: name is derived from the lookup key, not stored in NetworkInfo
        assert_eq!(info.chain_type, test_case.expected_chain_type);
        assert_eq!(info.chain_id, test_case.expected_chain_id);
        assert_eq!(info.mainnet, test_case.expected_mainnet);
        if let Some(expected_display_name) = test_case.expected_display_name {
            assert_eq!(info.display_name, expected_display_name);
        }
        if !test_case.expected_mainnet {
            assert!(
                info.is_testnet(),
                "Testnet network {} should return true for is_testnet()",
                test_case.name
            );
        }
    }
}

#[test]
fn test_get_network_unknown() {
    let network = get_network("unknown-network");
    assert!(network.is_none());

    let network = get_network("");
    assert!(network.is_none());
}

#[test]
fn test_is_evm_network() {
    let test_cases = vec![
        ("ethereum", true),
        ("base", true),
        ("base-sepolia", true),
        ("polygon", true),
        ("arbitrum", true),
        ("optimism", true),
        ("solana", false),
        ("solana-devnet", false),
        ("unknown", false),
    ];

    for (network, expected) in test_cases {
        assert_eq!(
            is_evm_network(network),
            expected,
            "is_evm_network({network}) should be {expected}"
        );
    }
}

#[test]
fn test_is_solana_network() {
    let test_cases = vec![
        ("solana", true),
        ("solana-devnet", true),
        ("ethereum", false),
        ("base", false),
        ("unknown", false),
    ];

    for (network, expected) in test_cases {
        assert_eq!(
            is_solana_network(network),
            expected,
            "is_solana_network({network}) should be {expected}"
        );
    }
}

#[test]
fn test_get_evm_chain_id() {
    let test_cases = vec![
        ("ethereum", Some(1)),
        ("base", Some(8453)),
        ("base-sepolia", Some(84532)),
        ("ethereum-sepolia", Some(11155111)),
        ("polygon", Some(137)),
        ("arbitrum", Some(42161)),
        ("optimism", Some(10)),
        ("avalanche", Some(43114)),
        ("avalanche-fuji", Some(43113)),
        // Solana networks don't have EVM chain IDs
        ("solana", None),
        ("unknown", None),
    ];

    for (network, expected_chain_id) in test_cases {
        assert_eq!(
            get_evm_chain_id(network),
            expected_chain_id,
            "get_evm_chain_id({network}) should be {expected_chain_id:?}"
        );
    }
}

#[test]
fn test_network_mainnet_flag() {
    let test_cases = vec![
        ("ethereum", true),
        ("base", true),
        ("solana", true),
        ("ethereum-sepolia", false),
        ("base-sepolia", false),
        ("solana-devnet", false),
    ];

    for (network, expected_mainnet) in test_cases {
        let info = get_network(network).unwrap();
        assert_eq!(
            info.mainnet, expected_mainnet,
            "Network {network} mainnet flag should be {expected_mainnet}"
        );
    }
}

#[test]
fn test_network_is_testnet_method() {
    let test_cases = vec![
        ("ethereum", false),
        ("base", false),
        ("ethereum-sepolia", true),
        ("base-sepolia", true),
    ];

    for (network, expected_is_testnet) in test_cases {
        let info = get_network(network).unwrap();
        assert_eq!(
            info.is_testnet(),
            expected_is_testnet,
            "Network {network} is_testnet() should be {expected_is_testnet}"
        );
    }
}

#[test]
fn test_all_evm_networks_have_chain_ids() {
    for network in Network::all() {
        let info = network.info();
        if info.chain_type == ChainType::Evm {
            assert!(
                info.chain_id.is_some(),
                "EVM network {network} should have a chain_id"
            );
        }
    }
}

#[test]
fn test_solana_networks_have_no_chain_ids() {
    for network in Network::all() {
        let info = network.info();
        if info.chain_type == ChainType::Solana {
            assert!(
                info.chain_id.is_none(),
                "Solana network {network} should not have a chain_id"
            );
        }
    }
}

// Token configuration tests

#[test]
fn test_get_token_decimals() {
    // Test success cases
    let success_cases = vec![
        ("solana", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", 6),
        (
            "solana-devnet",
            "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
            6,
        ),
        ("base", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", 6),
        // evm addresses not case sensitive
        ("base", "0x833589FCD6EDB6E08F4C7C32D4F71B54BDA02913", 6),
        ("ethereum", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", 6),
    ];

    for (network, token_address, expected_decimals) in success_cases {
        let result = get_token_decimals(network, token_address);
        assert!(
            result.is_ok(),
            "get_token_decimals({network}, {token_address}) should succeed"
        );
        assert_eq!(
            result.unwrap(),
            expected_decimals,
            "get_token_decimals({network}, {token_address}) should return {expected_decimals}"
        );
    }

    let error_cases = vec![
        ("base", "0x0000000000000000000000000000000000000000"),
        (
            "unknown-network",
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
        ),
        ("", ""),
    ];

    for (network, token_address) in error_cases {
        let result = get_token_decimals(network, token_address);
        assert!(
            result.is_err(),
            "get_token_decimals({network}, {token_address}) should return error"
        );
    }
}

#[test]
fn test_solana_addresses_are_case_sensitive() {
    let decimals = get_token_decimals("solana", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    assert!(decimals.is_ok());
    assert_eq!(decimals.unwrap(), 6);

    let decimals_wrong_case =
        get_token_decimals("solana", "epjfwdd5aufqssqem2qn1xzybapC8G4wEGGkZwyTDt1v");
    assert!(decimals_wrong_case.is_err());
}

#[test]
fn test_chain_type_equality() {
    assert_eq!(ChainType::Evm, ChainType::Evm);
    assert_eq!(ChainType::Solana, ChainType::Solana);
    assert_eq!(ChainType::Tempo, ChainType::Tempo);
    assert_ne!(ChainType::Evm, ChainType::Solana);
    assert_ne!(ChainType::Evm, ChainType::Tempo);
    assert_ne!(ChainType::Solana, ChainType::Tempo);
}

// Tempo network tests

#[test]
fn test_get_tempo_network() {
    let network = get_network("tempo");
    assert!(network.is_some(), "tempo should exist");

    let info = network.unwrap();
    assert_eq!(info.chain_type, ChainType::Tempo);
    assert_eq!(info.chain_id, Some(4217));
    assert!(info.mainnet);
    assert!(!info.is_testnet());
    assert_eq!(info.display_name, "Tempo");
}

#[test]
fn test_get_tempo_moderato_network() {
    let network = get_network("tempo-moderato");
    assert!(network.is_some(), "tempo-moderato should exist");

    let info = network.unwrap();
    assert_eq!(info.chain_type, ChainType::Tempo);
    assert_eq!(info.chain_id, Some(42431));
    assert!(!info.mainnet);
    assert!(info.is_testnet());
    assert_eq!(info.display_name, "Tempo Moderato");
}

#[test]
fn test_is_tempo_network() {
    assert!(is_tempo_network("tempo"));
    assert!(is_tempo_network("tempo-moderato"));
    assert!(is_tempo_network("eip155:4217"));
    assert!(is_tempo_network("eip155:42431"));
    assert!(!is_tempo_network("base"));
    assert!(!is_tempo_network("solana"));
    assert!(!is_tempo_network("unknown"));
}

#[test]
fn test_tempo_network_alias() {
    assert_eq!(resolve_network_alias("eip155:4217"), "tempo");
    assert_eq!(resolve_network_alias("eip155:42431"), "tempo-moderato");
}

#[test]
fn test_tempo_chain_id() {
    assert_eq!(get_evm_chain_id("tempo"), Some(4217));
    assert_eq!(get_evm_chain_id("eip155:4217"), Some(4217));
    assert_eq!(get_evm_chain_id("tempo-moderato"), Some(42431));
    assert_eq!(get_evm_chain_id("eip155:42431"), Some(42431));
}

#[test]
fn test_tempo_is_not_evm_or_solana() {
    assert!(!is_evm_network("tempo-moderato"));
    assert!(!is_solana_network("tempo-moderato"));
}

#[test]
fn test_tempo_network_enum() {
    let tempo: Network = "tempo".parse().unwrap();
    assert_eq!(tempo, Network::Tempo);
    assert_eq!(tempo.as_str(), "tempo");
    assert_eq!(tempo.chain_type(), ChainType::Tempo);
    assert!(!tempo.is_testnet());
    assert!(tempo.is_mainnet());

    let tempo_moderato: Network = "tempo-moderato".parse().unwrap();
    assert_eq!(tempo_moderato, Network::TempoModerato);
    assert_eq!(tempo_moderato.as_str(), "tempo-moderato");
    assert_eq!(tempo_moderato.chain_type(), ChainType::Tempo);
    assert!(tempo_moderato.is_testnet());
    assert!(!tempo_moderato.is_mainnet());
}

#[test]
fn test_tempo_has_explorer_url() {
    // Mainnet
    let info = get_network("tempo").unwrap();
    assert!(info.explorer_url.is_some());

    let tx_url = info.tx_url("0xabc123");
    assert!(tx_url.is_some());
    assert!(tx_url.unwrap().contains("explore.tempo.xyz/tx/0xabc123"));

    let addr_url = info.address_url("0x1234");
    assert!(addr_url.is_some());
    assert!(addr_url.unwrap().contains("explore.tempo.xyz/address/0x1234"));

    // Testnet (shared explorer)
    let info = get_network("tempo-moderato").unwrap();
    assert!(info.explorer_url.is_some());
    assert!(info.tx_url("0xabc123").unwrap().contains("explore.tempo.xyz/tx/0xabc123"));
}

#[test]
fn test_tempo_default_token_config() {
    // Tempo mainnet uses USDC
    let tempo = Network::Tempo;
    let config = tempo.default_token_config();
    assert!(config.is_some());
    let token = config.unwrap();
    assert_eq!(token.currency.symbol, "USDC");
    assert_eq!(token.currency.decimals, 6);
    assert_eq!(token.address, "0x20C000000000000000000000b9537d11c60E8b50");

    // Tempo mainnet has USDC config directly
    assert!(tempo.usdc_config().is_some());

    // Tempo Moderato should NOT have USDC
    let moderato = Network::TempoModerato;
    assert!(moderato.usdc_config().is_none());

    // Tempo Moderato should have pathUSD as default
    let config = moderato.default_token_config();
    assert!(config.is_some());
    let token = config.unwrap();
    assert_eq!(token.currency.symbol, "pathUSD");
    assert_eq!(token.currency.decimals, 6);
    assert_eq!(token.address, "0x20c0000000000000000000000000000000000000");
}

#[test]
fn test_tempo_token_decimals() {
    // USDC on Tempo mainnet
    let decimals = get_token_decimals(
        "tempo",
        "0x20c000000000000000000000b9537d11c60e8b50",
    );
    assert!(decimals.is_ok());
    assert_eq!(decimals.unwrap(), 6);

    // pathUSD on Tempo Moderato
    let decimals = get_token_decimals(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000000",
    );
    assert!(decimals.is_ok());
    assert_eq!(decimals.unwrap(), 6);

    // alphaUSD on Tempo Moderato
    let decimals = get_token_decimals(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000001",
    );
    assert!(decimals.is_ok());
    assert_eq!(decimals.unwrap(), 6);
}

#[test]
fn test_tempo_token_symbols() {
    // Tempo mainnet USDC
    let symbol = get_token_symbol(
        "tempo",
        "0x20c000000000000000000000b9537d11c60e8b50",
    );
    assert_eq!(symbol, Some("USDC"));

    // Tempo Moderato pathUSD
    let symbol = get_token_symbol(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000000",
    );
    assert_eq!(symbol, Some("pathUSD"));

    let symbol = get_token_symbol(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000001",
    );
    assert_eq!(symbol, Some("alphaUSD"));

    let symbol = get_token_symbol(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000002",
    );
    assert_eq!(symbol, Some("betaUSD"));

    let symbol = get_token_symbol(
        "tempo-moderato",
        "0x20c0000000000000000000000000000000000003",
    );
    assert_eq!(symbol, Some("thetaUSD"));
}

#[test]
fn test_tempo_networks_have_chain_ids() {
    for network in Network::all() {
        let info = network.info();
        if info.chain_type == ChainType::Tempo {
            assert!(
                info.chain_id.is_some(),
                "Tempo network {} should have a chain_id",
                network
            );
        }
    }
}

#[test]
fn test_network_by_chain_type_tempo() {
    let tempo_networks = Network::by_chain_type(ChainType::Tempo, None);
    assert!(!tempo_networks.is_empty());
    assert!(tempo_networks.contains(&Network::Tempo));
    assert!(tempo_networks.contains(&Network::TempoModerato));

    // Filtering by name
    let filtered = Network::by_chain_type(ChainType::Tempo, Some("tempo"));
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], Network::Tempo);

    let filtered = Network::by_chain_type(ChainType::Tempo, Some("tempo-moderato"));
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], Network::TempoModerato);

    // Filtering by non-existent name
    let filtered = Network::by_chain_type(ChainType::Tempo, Some("nonexistent"));
    assert!(filtered.is_empty());
}
