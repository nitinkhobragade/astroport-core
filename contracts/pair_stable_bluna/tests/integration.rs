use astroport::asset::{Asset, AssetInfo, PairInfo};
use astroport::factory::{
    ExecuteMsg as FactoryExecuteMsg, InstantiateMsg as FactoryInstantiateMsg, PairConfig, PairType,
    QueryMsg as FactoryQueryMsg,
};
use astroport::pair::{
    ConfigResponse, CumulativePricesResponse, Cw20HookMsg, InstantiateMsg, QueryMsg, TWAP_PRECISION,
};

use astroport::pair_stable_bluna::{
    ExecuteMsg, StablePoolConfig, StablePoolParams, StablePoolUpdateParams,
};

use astroport::token::InstantiateMsg as TokenInstantiateMsg;
use astroport_pair_stable_bluna::math::{MAX_AMP, MAX_AMP_CHANGE, MIN_AMP_CHANGING_TIME};
use cosmwasm_std::testing::{mock_env, MockApi, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    attr, from_binary, to_binary, Addr, Coin, Decimal, QueryRequest, StdResult, Uint128, WasmQuery,
};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg, MinterResponse};
use terra_multi_test::{AppBuilder, BankKeeper, ContractWrapper, Executor, TerraApp, TerraMock};

const OWNER: &str = "owner";

fn mock_app() -> TerraApp {
    let env = mock_env();
    let api = MockApi::default();
    let bank = BankKeeper::new();
    let storage = MockStorage::new();
    let custom = TerraMock::luna_ust_case();

    AppBuilder::new()
        .with_api(api)
        .with_block(env.block)
        .with_bank(bank)
        .with_storage(storage)
        .with_custom(custom)
        .build()
}
fn store_token_code(app: &mut TerraApp) -> u64 {
    let astro_token_contract = Box::new(ContractWrapper::new_with_empty(
        astroport_token::contract::execute,
        astroport_token::contract::instantiate,
        astroport_token::contract::query,
    ));

    app.store_code(astro_token_contract)
}

fn store_pair_code(app: &mut TerraApp) -> u64 {
    let pair_contract = Box::new(
        ContractWrapper::new_with_empty(
            astroport_pair_stable_bluna::contract::execute,
            astroport_pair_stable_bluna::contract::instantiate,
            astroport_pair_stable_bluna::contract::query,
        )
        .with_reply_empty(astroport_pair_stable_bluna::contract::reply),
    );

    app.store_code(pair_contract)
}

fn store_factory_code(app: &mut TerraApp) -> u64 {
    let factory_contract = Box::new(
        ContractWrapper::new_with_empty(
            astroport_factory::contract::execute,
            astroport_factory::contract::instantiate,
            astroport_factory::contract::query,
        )
        .with_reply_empty(astroport_factory::contract::reply),
    );

    app.store_code(factory_contract)
}

fn store_whitelist_code(app: &mut TerraApp) -> u64 {
    let whitelist_contract = Box::new(ContractWrapper::new_with_empty(
        astroport_whitelist::contract::execute,
        astroport_whitelist::contract::instantiate,
        astroport_whitelist::contract::query,
    ));

    app.store_code(whitelist_contract)
}

fn instantiate_factory(
    mut router: &mut TerraApp,
    owner: &Addr,
    token_code_id: u64,
    pair_code_id: u64,
) -> Addr {
    let factory_code_id = store_factory_code(&mut router);
    let whitelist_code_id = store_whitelist_code(&mut router);

    let init_msg = FactoryInstantiateMsg {
        fee_address: None,
        pair_configs: vec![PairConfig {
            code_id: pair_code_id,
            maker_fee_bps: 0,
            total_fee_bps: 0,
            pair_type: PairType::Stable {},
            is_disabled: None,
        }],
        token_code_id,
        generator_address: Some(MOCK_CONTRACT_ADDR.to_string()),
        owner: owner.to_string(),
        whitelist_code_id,
    };

    router
        .instantiate_contract(
            factory_code_id,
            owner.clone(),
            &init_msg,
            &[],
            "FACTORY",
            None,
        )
        .unwrap()
}

fn instantiate_bluna_contracts(router: &mut TerraApp, sender: &Addr) -> (Addr, Addr, Addr) {
    let bluna_hub_contract = Box::new(ContractWrapper::new_with_empty(
        anchor_basset_hub::contract::execute,
        anchor_basset_hub::contract::instantiate,
        anchor_basset_hub::contract::query,
    ));

    let bluna_hub_code_id = router.store_code(bluna_hub_contract);

    let bluna_hub_init_msg = anchor_basset::hub::InstantiateMsg {
        epoch_period: 0,
        er_threshold: Decimal::zero(),
        peg_recovery_fee: Decimal::zero(),
        reward_denom: "".to_string(),
        unbonding_period: 0,
        underlying_coin_denom: "".to_string(),
        validator: "".to_string(),
    };

    let bluna_hub_instance = router
        .instantiate_contract(
            bluna_hub_code_id,
            sender.clone(),
            &bluna_hub_init_msg,
            &[],
            "BLUNA HUB",
            None,
        )
        .unwrap();

    let bluna_rewarder_contract = Box::new(ContractWrapper::new(
        anchor_basset_reward::contract::execute,
        anchor_basset_reward::contract::instantiate,
        anchor_basset_reward::contract::query,
    ));

    let bluna_reward_code_id = router.store_code(bluna_rewarder_contract);

    let bluna_reward_init_msg = anchor_basset::reward::InstantiateMsg {
        hub_contract: bluna_hub_instance.to_string(),
        reward_denom: "uusd".to_string(),
    };

    let bluna_reward_instance = router
        .instantiate_contract(
            bluna_reward_code_id,
            sender.clone(),
            &bluna_reward_init_msg,
            &[],
            "BLUNA REWARD",
            None,
        )
        .unwrap();

    let bluna_token_contract = Box::new(ContractWrapper::new_with_empty(
        anchor_basset_token::contract::execute,
        anchor_basset_token::contract::instantiate,
        anchor_basset_token::contract::query,
    ));

    let bluna_token_code_id = router.store_code(bluna_token_contract);

    let bluna_token_init_msg = cw20_legacy::msg::InstantiateMsg {
        decimals: 6,
        initial_balances: vec![],
        mint: Some(MinterResponse {
            cap: None,
            minter: bluna_hub_instance.to_string(),
        }),
        name: "Bluna".to_string(),
        symbol: "BLUNA".to_string(),
    };

    let bluna_token_instance = router
        .instantiate_contract(
            bluna_token_code_id,
            sender.clone(),
            &bluna_token_init_msg,
            &[],
            "BLUNA TOKEN",
            None,
        )
        .unwrap();

    (
        bluna_hub_instance,
        bluna_reward_instance,
        bluna_token_instance,
    )
}

fn instantiate_pair(
    mut router: &mut TerraApp,
    owner: &Addr,
    token_contract_code_id: u64,
    pair_contract_code_id: u64,
    factory_addr: &Addr,
    bluna_token: &Addr,
    bluna_rewarder: &Addr,
) -> Addr {
    let msg = InstantiateMsg {
        asset_infos: [
            AssetInfo::Token {
                contract_addr: bluna_token.clone(),
            },
            AssetInfo::NativeToken {
                denom: "uluna".to_string(),
            },
        ],
        token_code_id: token_contract_code_id,
        factory_addr: factory_addr.clone(),
        init_params: None,
    };

    let resp = router
        .instantiate_contract(
            pair_contract_code_id,
            owner.clone(),
            &msg,
            &[],
            String::from("PAIR"),
            None,
        )
        .unwrap_err();
    assert_eq!("You need to provide init params", resp.to_string());

    let msg = InstantiateMsg {
        asset_infos: [
            AssetInfo::Token {
                contract_addr: bluna_token.clone(),
            },
            AssetInfo::NativeToken {
                denom: "uluna".to_string(),
            },
        ],
        token_code_id: token_contract_code_id,
        factory_addr: factory_addr.clone(),
        init_params: Some(
            to_binary(&StablePoolParams {
                amp: 100,
                bluna_rewarder: bluna_rewarder.to_string(),
            })
            .unwrap(),
        ),
    };

    let pair = router
        .instantiate_contract(
            pair_contract_code_id,
            owner.clone(),
            &msg,
            &[],
            String::from("PAIR"),
            None,
        )
        .unwrap();

    pair
}

#[test]
fn test_provide_and_withdraw_liquidity() {
    let owner = Addr::unchecked(OWNER);
    let alice_address = Addr::unchecked("alice");
    let mut router = mock_app();

    // Set alice balances
    router
        .init_bank_balance(
            &alice_address,
            vec![Coin {
                denom: "uluna".to_string(),
                amount: Uint128::new(200u128),
            }],
        )
        .unwrap();

    let token_code_id = store_token_code(&mut router);
    let pair_code_id = store_pair_code(&mut router);

    let factory_instance = instantiate_factory(&mut router, &owner, token_code_id, pair_code_id);

    let (bluna_hub_instance, bluna_reward_instance, bluna_token_instance) =
        instantiate_bluna_contracts(&mut router, &owner);

    // Init pair
    let pair_instance = instantiate_pair(
        &mut router,
        &owner,
        token_code_id,
        pair_code_id,
        &factory_instance,
        &bluna_token_instance,
        &bluna_reward_instance,
    );

    let res: Result<PairInfo, _> = router.wrap().query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: pair_instance.to_string(),
        msg: to_binary(&QueryMsg::Pair {}).unwrap(),
    }));
    let res = res.unwrap();

    assert_eq!(
        res.asset_infos,
        [
            AssetInfo::Token {
                contract_addr: bluna_token_instance.clone(),
            },
            AssetInfo::NativeToken {
                denom: "uluna".to_string(),
            },
        ],
    );

    router
        .init_bank_balance(
            &pair_instance,
            vec![Coin {
                denom: "uluna".to_string(),
                amount: Uint128::new(100u128),
            }],
        )
        .unwrap();

    // Provide liquidity
    let (msg, coins) = provide_liquidity_msg(Uint128::new(100), Uint128::new(100), None);
    let res = router
        .execute_contract(alice_address.clone(), pair_instance.clone(), &msg, &coins)
        .unwrap();

    assert_eq!(
        res.events[1].attributes[1],
        attr("action", "provide_liquidity")
    );
    assert_eq!(res.events[1].attributes[3], attr("receiver", "alice"),);
    assert_eq!(
        res.events[1].attributes[4],
        attr("assets", "100uusd, 100uluna")
    );
    assert_eq!(
        res.events[1].attributes[5],
        attr("share", 100u128.to_string())
    );
    assert_eq!(res.events[3].attributes[1], attr("action", "mint"));
    assert_eq!(res.events[3].attributes[2], attr("to", "alice"));
    assert_eq!(
        res.events[3].attributes[3],
        attr("amount", 100u128.to_string())
    );

    // Provide liquidity for receiver
    let (msg, coins) = provide_liquidity_msg(
        Uint128::new(100),
        Uint128::new(100),
        Some("bob".to_string()),
    );
    let res = router
        .execute_contract(alice_address.clone(), pair_instance.clone(), &msg, &coins)
        .unwrap();

    assert_eq!(
        res.events[1].attributes[1],
        attr("action", "provide_liquidity")
    );
    assert_eq!(res.events[1].attributes[3], attr("receiver", "bob"),);
    assert_eq!(
        res.events[1].attributes[4],
        attr("assets", "100uusd, 100uluna")
    );
    assert_eq!(
        res.events[1].attributes[5],
        attr("share", 50u128.to_string())
    );
    assert_eq!(res.events[3].attributes[1], attr("action", "mint"));
    assert_eq!(res.events[3].attributes[2], attr("to", "bob"));
    assert_eq!(res.events[3].attributes[3], attr("amount", 50.to_string()));
}

fn provide_liquidity_msg(
    bluna_token: &Addr,
    bluna_amount: Uint128,
    uluna_amount: Uint128,
    receiver: Option<String>,
) -> (ExecuteMsg, [Coin; 2]) {
    let msg = ExecuteMsg::ProvideLiquidity {
        assets: [
            Asset {
                info: AssetInfo::Token {
                    contract_addr: bluna_token.clone(),
                },
                amount: bluna_amount.clone(),
            },
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "uluna".to_string(),
                },
                amount: uluna_amount.clone(),
            },
        ],
        slippage_tolerance: None,
        auto_stake: None,
        receiver,
    };

    let coins = [Coin {
        denom: "uluna".to_string(),
        amount: uluna_amount.clone(),
    }];

    (msg, coins)
}

#[test]
fn test_if_twap_is_calculated_correctly_when_pool_idles() {
    let mut app = mock_app();

    let user1 = Addr::unchecked("user1");

    app.init_bank_balance(
        &user1,
        vec![
            Coin {
                denom: "uusd".to_string(),
                amount: Uint128::new(4000000_000000),
            },
            Coin {
                denom: "uluna".to_string(),
                amount: Uint128::new(2000000_000000),
            },
        ],
    )
    .unwrap();

    let token_code_id = store_token_code(&mut app);
    let pair_code_id = store_pair_code(&mut app);

    let factory_instance = instantiate_factory(&mut app, &user1, token_code_id, pair_code_id);

    let (bluna_hub_instance, bluna_reward_instance, bluna_token_instance) =
        instantiate_bluna_contracts(&mut app, &user1);

    // instantiate pair
    let pair_instance = instantiate_pair(
        &mut app,
        &user1,
        token_code_id,
        pair_code_id,
        &factory_instance,
        &bluna_token_instance,
        &bluna_reward_instance,
    );

    // provide liquidity, accumulators are empty
    let (msg, coins) = provide_liquidity_msg(
        Uint128::new(1000000_000000),
        Uint128::new(1000000_000000),
        None,
    );
    app.execute_contract(user1.clone(), pair_instance.clone(), &msg, &coins)
        .unwrap();

    const BLOCKS_PER_DAY: u64 = 17280;
    const ELAPSED_SECONDS: u64 = BLOCKS_PER_DAY * 5;

    // a day later
    app.update_block(|b| {
        b.height += BLOCKS_PER_DAY;
        b.time = b.time.plus_seconds(ELAPSED_SECONDS);
    });

    // provide liquidity, accumulators firstly filled with the same prices
    let (msg, coins) = provide_liquidity_msg(
        Uint128::new(3000000_000000),
        Uint128::new(1000000_000000),
        None,
    );
    app.execute_contract(user1.clone(), pair_instance.clone(), &msg, &coins)
        .unwrap();

    // get current twap accumulator values
    let msg = QueryMsg::CumulativePrices {};
    let cpr_old: CumulativePricesResponse =
        app.wrap().query_wasm_smart(&pair_instance, &msg).unwrap();

    // a day later
    app.update_block(|b| {
        b.height += BLOCKS_PER_DAY;
        b.time = b.time.plus_seconds(ELAPSED_SECONDS);
    });

    // get current twap accumulator values, it should be added up by the query method with new 2/1 ratio
    let msg = QueryMsg::CumulativePrices {};
    let cpr_new: CumulativePricesResponse =
        app.wrap().query_wasm_smart(&pair_instance, &msg).unwrap();

    let twap0 = cpr_new.price0_cumulative_last - cpr_old.price0_cumulative_last;
    let twap1 = cpr_new.price1_cumulative_last - cpr_old.price1_cumulative_last;

    // Prices weren't changed for the last day, uusd amount in pool = 4000000_000000, uluna = 2000000_000000
    let price_precision = Uint128::from(10u128.pow(TWAP_PRECISION.into()));
    assert_eq!(twap0 / price_precision, Uint128::new(85684)); // 1.008356286 * ELAPSED_SECONDS (86400)
    assert_eq!(twap1 / price_precision, Uint128::new(87121)); //   0.991712963 * ELAPSED_SECONDS
}

#[test]
fn create_pair_with_same_assets() {
    let mut router = mock_app();
    let owner = Addr::unchecked("owner");

    let token_contract_code_id = store_token_code(&mut router);
    let pair_contract_code_id = store_pair_code(&mut router);
    let whitelist_code_id = store_whitelist_code(&mut router);

    let factory_code_id = store_factory_code(&mut router);

    let init_msg = FactoryInstantiateMsg {
        fee_address: None,
        pair_configs: vec![PairConfig {
            code_id: pair_contract_code_id,
            maker_fee_bps: 0,
            total_fee_bps: 0,
            pair_type: PairType::Stable {},
            is_disabled: None,
        }],
        token_code_id: token_contract_code_id,
        generator_address: Some(String::from("generator")),
        owner: String::from("owner0000"),
        whitelist_code_id,
    };

    let factory_instance = router
        .instantiate_contract(
            factory_code_id,
            owner.clone(),
            &init_msg,
            &[],
            "FACTORY",
            None,
        )
        .unwrap();

    let msg = InstantiateMsg {
        asset_infos: [
            AssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            AssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
        ],
        token_code_id: token_contract_code_id,
        factory_addr: factory_instance.clone(),
        init_params: None,
    };

    let resp = router
        .instantiate_contract(
            pair_contract_code_id,
            owner.clone(),
            &msg,
            &[],
            String::from("PAIR"),
            None,
        )
        .unwrap_err();

    assert_eq!(resp.to_string(), "Doubling assets in asset infos")
}

#[test]
fn update_pair_config() {
    let mut router = mock_app();
    let owner = Addr::unchecked("owner");

    let token_contract_code_id = store_token_code(&mut router);
    let pair_contract_code_id = store_pair_code(&mut router);
    let whitelist_code_id = store_whitelist_code(&mut router);

    let factory_code_id = store_factory_code(&mut router);

    let init_msg = FactoryInstantiateMsg {
        fee_address: None,
        pair_configs: vec![],
        token_code_id: token_contract_code_id,
        generator_address: Some(String::from("generator")),
        owner: owner.to_string(),
        whitelist_code_id,
    };

    let factory_instance = router
        .instantiate_contract(
            factory_code_id,
            owner.clone(),
            &init_msg,
            &[],
            "FACTORY",
            None,
        )
        .unwrap();

    let msg = InstantiateMsg {
        asset_infos: [
            AssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            AssetInfo::NativeToken {
                denom: "uluna".to_string(),
            },
        ],
        token_code_id: token_contract_code_id,
        factory_addr: factory_instance,
        init_params: Some(
            to_binary(&StablePoolParams {
                amp: 100,
                bluna_rewarder: "bluna_rewarder".to_string(),
            })
            .unwrap(),
        ),
    };

    let pair = router
        .instantiate_contract(
            pair_contract_code_id,
            owner.clone(),
            &msg,
            &[],
            String::from("PAIR"),
            None,
        )
        .unwrap();

    let res: ConfigResponse = router
        .wrap()
        .query_wasm_smart(pair.clone(), &QueryMsg::Config {})
        .unwrap();

    let params: StablePoolConfig = from_binary(&res.params.unwrap()).unwrap();

    assert_eq!(params.amp, Decimal::from_ratio(100u32, 1u32));

    //Start changing amp with incorrect next amp
    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StartChangingAmp {
            next_amp: MAX_AMP + 1,
            next_amp_time: router.block_info().time.seconds(),
        })
        .unwrap(),
    };

    let resp = router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap_err();

    assert_eq!(
        resp.to_string(),
        format!(
            "Amp coefficient must be greater than 0 and less than or equal to {}",
            MAX_AMP
        )
    );

    //Start changing amp with big difference between the old and new amp value
    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StartChangingAmp {
            next_amp: 100 * MAX_AMP_CHANGE + 1,
            next_amp_time: router.block_info().time.seconds(),
        })
        .unwrap(),
    };

    let resp = router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap_err();

    assert_eq!(
        resp.to_string(),
        format!(
            "The difference between the old and new amp value must not exceed {} times",
            MAX_AMP_CHANGE
        )
    );

    //Start changing amp earlier than the MIN_AMP_CHANGING_TIME has elapsed
    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StartChangingAmp {
            next_amp: 250,
            next_amp_time: router.block_info().time.seconds(),
        })
        .unwrap(),
    };

    let resp = router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap_err();

    assert_eq!(
        resp.to_string(),
        format!(
            "Amp coefficient cannot be changed more often than once per {} seconds",
            MIN_AMP_CHANGING_TIME
        )
    );

    // Start increasing amp
    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME);
    });

    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StartChangingAmp {
            next_amp: 250,
            next_amp_time: router.block_info().time.seconds() + MIN_AMP_CHANGING_TIME,
        })
        .unwrap(),
    };

    router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap();

    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME / 2);
    });

    let res: ConfigResponse = router
        .wrap()
        .query_wasm_smart(pair.clone(), &QueryMsg::Config {})
        .unwrap();

    let params: StablePoolConfig = from_binary(&res.params.unwrap()).unwrap();

    assert_eq!(params.amp, Decimal::from_ratio(175u32, 1u32));

    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME / 2);
    });

    let res: ConfigResponse = router
        .wrap()
        .query_wasm_smart(pair.clone(), &QueryMsg::Config {})
        .unwrap();

    let params: StablePoolConfig = from_binary(&res.params.unwrap()).unwrap();

    assert_eq!(params.amp, Decimal::from_ratio(250u32, 1u32));

    // Start decreasing amp
    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME);
    });

    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StartChangingAmp {
            next_amp: 50,
            next_amp_time: router.block_info().time.seconds() + MIN_AMP_CHANGING_TIME,
        })
        .unwrap(),
    };

    router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap();

    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME / 2);
    });

    let res: ConfigResponse = router
        .wrap()
        .query_wasm_smart(pair.clone(), &QueryMsg::Config {})
        .unwrap();

    let params: StablePoolConfig = from_binary(&res.params.unwrap()).unwrap();

    assert_eq!(params.amp, Decimal::from_ratio(150u32, 1u32));

    // Stop changing amp
    let msg = ExecuteMsg::UpdateConfig {
        params: to_binary(&StablePoolUpdateParams::StopChangingAmp {}).unwrap(),
    };

    router
        .execute_contract(owner.clone(), pair.clone(), &msg, &[])
        .unwrap();

    router.update_block(|b| {
        b.time = b.time.plus_seconds(MIN_AMP_CHANGING_TIME / 2);
    });

    let res: ConfigResponse = router
        .wrap()
        .query_wasm_smart(pair.clone(), &QueryMsg::Config {})
        .unwrap();

    let params: StablePoolConfig = from_binary(&res.params.unwrap()).unwrap();

    assert_eq!(params.amp, Decimal::from_ratio(150u32, 1u32));
}
