//! Integration tests for Lombard Finance LBTC descriptor.
//! Real on-chain transactions from 0x8236a87084f8b84306f72007f36f2618a5634494 (Ethereum mainnet).

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{
    CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource,
};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, TransactionContext};

const CONTRACT: &str = "0x8236a87084f8b84306f72007f36f2618a5634494";

fn load_descriptor() -> Descriptor {
    let path = format!(
        "{}/tests/fixtures/calldata-lbtc-mainnet.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn wrap_rd(descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id,
        address: address.to_lowercase(),
    }]
}

fn lbtc_token_source() -> CompositeDataProvider {
    let mut custom = StaticTokenSource::new();
    // LBTC: Lombard Staked BTC — 8 decimals (resolved via Alchemy alchemy_getTokenMetadata)
    custom.insert(
        1,
        "0x8236a87084f8b84306f72007f36f2618a5634494",
        TokenMeta {
            symbol: "LBTC".to_string(),
            decimals: 8,
            name: "Lombard Staked BTC".to_string(),
        },
    );
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).expect("valid hex")
}

fn get_item(model: &clear_signing::DisplayModel, label: &str) -> String {
    for entry in &model.entries {
        if let DisplayEntry::Item(item) = entry {
            if item.label == label {
                return item.value.clone();
            }
        }
    }
    panic!("no entry with label '{label}' in {:?}", model.entries);
}

// --- redeemForBtc ---
// Tx: 0x1864ab1d8bcc61d190ae6ea78611fdff68907971fcce41481aea16227bee0f60
// P2TR scriptPubKey, amount=3450700 (0.034507 LBTC)
#[tokio::test]
async fn lbtc_redeem_for_btc_p2tr() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("30b93d850000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000034a74c000000000000000000000000000000000000000000000000000000000000002251203ecfea2fc36feac3446a5e81629d316cf1e0b5226cdb134b35100502fb36c97f000000000000000000000000000000000000000000000000000000000000");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x743a88c4dd913693e2306be9c1d3d00f3e3ab52d"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Redeem BTC");
    assert_eq!(result.owner, Some("Lombard Finance".to_string()));
    assert_eq!(get_item(&result, "Amount to Burn"), "0.034507 LBTC");
    assert_eq!(
        get_item(&result, "ScriptPubKey (BTC)"),
        "0x51203ecfea2fc36feac3446a5e81629d316cf1e0b5226cdb134b35100502fb36c97f"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0x7e2b56c6038faff4a7a921ae3e6621570958afaa9814e254b70734f541c45f9e
// P2WPKH scriptPubKey, amount=629476 (0.00629476 LBTC)
#[tokio::test]
async fn lbtc_redeem_for_btc_p2wpkh() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("30b93d8500000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000099ae40000000000000000000000000000000000000000000000000000000000000016001437d5dc33584d8829b3503f2046095f3978f9d02000000000000000000000");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0xa162f3b352a54d1e2c891b7a76aa84cf99399665"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Redeem BTC");
    assert_eq!(get_item(&result, "Amount to Burn"), "0.00629476 LBTC");
    assert_eq!(
        get_item(&result, "ScriptPubKey (BTC)"),
        "0x001437d5dc33584d8829b3503f2046095f3978f9d020"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0x9400d5dea6114bd894e53d9f09f24eeabc815ec99f18869c80dfbbef513add85
// P2WPKH scriptPubKey, amount=1499700 (0.014997 LBTC)
#[tokio::test]
async fn lbtc_redeem_for_btc_p2wpkh_2() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("30b93d850000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000016e234000000000000000000000000000000000000000000000000000000000000001600147e91f438c184df239a78d4e47fa00001affd326800000000000000000000");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0xea24860db3a06039b9797d7131365e16b66d5a3a"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Redeem BTC");
    assert_eq!(get_item(&result, "Amount to Burn"), "0.014997 LBTC");
    assert_eq!(
        get_item(&result, "ScriptPubKey (BTC)"),
        "0x00147e91f438c184df239a78d4e47fa00001affd3268"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// --- approve ---
// Tx: 0x7c0e5352232c5020cc0fb673663e6a8445536fcf61d90d94059618449a7ee40b
// value=12988784 (0.12988784 LBTC)
#[tokio::test]
async fn lbtc_approve_small_amount() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("095ea7b30000000000000000000000005f46d540b6ed704c3c8789105f30e075aa9007260000000000000000000000000000000000000000000000000000000000c63170");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x4bb3aa55c16a58a799c148559fd1a4205b16f381"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Approve");
    assert_eq!(result.owner, Some("Lombard Finance".to_string()));
    assert_eq!(get_item(&result, "Amount to Approve"), "0.12988784 LBTC");
    assert_eq!(
        get_item(&result, "Spender"),
        "0x5f46d540b6eD704C3c8789105F30E075AA900726"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0xa0fe4f792e5c532995673b804cd3cf9bbe37b92bbe9316e39f565e70c4941556
// value=219171239 (2.19171239 LBTC), spender=Uniswap v3 NonfungiblePositionManager
#[tokio::test]
async fn lbtc_approve_uniswap_v3() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("095ea7b3000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88000000000000000000000000000000000000000000000000000000000d1049a7");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x7e6bee8157aceb2cbaaa74868f99a369f9e5d4c1"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Approve");
    assert_eq!(get_item(&result, "Amount to Approve"), "2.19171239 LBTC");
    assert_eq!(
        get_item(&result, "Spender"),
        "0xC36442b4a4522E871399CD717aBDD847Ab11FE88"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0xbaab87544e796a22abf354d891a5055b7c155c420b709a1a669570e09037f338
// value=59000000 (0.59 LBTC)
#[tokio::test]
async fn lbtc_approve_round_amount() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("095ea7b30000000000000000000000006131b5fae19ea4f9d964eac0408e4408b66337b500000000000000000000000000000000000000000000000000000000038444c0");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0xab9e01aea5ac65b0812f1e0d2ee89d7a6e7d25c9"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Approve");
    assert_eq!(get_item(&result, "Amount to Approve"), "0.59 LBTC");
    assert_eq!(
        get_item(&result, "Spender"),
        "0x6131B5fae19EA4f9D964eAc0408E4408b66337b5"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// --- transfer ---
// Tx: 0xe5b88bbeddc9c663b6c90a7ce912eee3d4544112d4a904f5613dfa876f85fc21
// value=199406343 (1.99406343 LBTC)
#[tokio::test]
async fn lbtc_transfer_nearly_2_btc() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("a9059cbb0000000000000000000000001cd026ea8b42a376abc5d471fd8d6276a7913b93000000000000000000000000000000000000000000000000000000000be2b307");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x066f3a28d702e9832b9666fd1ff5dd4baaa1baa3"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Send");
    assert_eq!(result.owner, Some("Lombard Finance".to_string()));
    assert_eq!(get_item(&result, "Amount to Send"), "1.99406343 LBTC");
    assert_eq!(
        get_item(&result, "Recipient"),
        "0x1CD026EA8B42A376ABC5d471Fd8D6276a7913B93"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0x15cf154f4907a9217b802e30403ad600c01b786f48e471b299235f936cc861df
// value=11400099998 (114.00099998 LBTC) — large transfer
#[tokio::test]
async fn lbtc_transfer_large_amount() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("a9059cbb00000000000000000000000052d757b42719ce13c92edd87fa14ccc0898c3f9b00000000000000000000000000000000000000000000000000000002a77fb89e");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0xf50623b782fb0c7cd3966b20f6ecb7d791a36ec6"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Send");
    assert_eq!(get_item(&result, "Amount to Send"), "114.00099998 LBTC");
    assert_eq!(
        get_item(&result, "Recipient"),
        "0x52D757B42719Ce13C92EdD87fA14ccc0898c3F9B"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// Tx: 0x79aed93050daa27083540a041f61c47430148df727be458db4853e1fd846a9f9
// value=6889 (0.00006889 LBTC) — dust transfer
#[tokio::test]
async fn lbtc_transfer_dust_amount() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("a9059cbb000000000000000000000000f89d7b9c864f589bbf53a82105107622b35eaa400000000000000000000000000000000000000000000000000000000000001ae9");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0xf440139a62b2b939699c5b3e09f88e40464ab9bc"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Send");
    assert_eq!(get_item(&result, "Amount to Send"), "0.00006889 LBTC");
    assert_eq!(
        get_item(&result, "Recipient"),
        "0xf89d7b9c864f589bbF53a82105107622B35EaA40"
    );
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}

// --- mint ---
// Tx: 0x5edc6818533014126572c0637dae3a92691fb000966a3a30e4f060f6d3c70333
// rawPayload + proof shown as raw hex blobs
#[tokio::test]
async fn lbtc_mint_raw_payload() {
    let descriptors = wrap_rd(load_descriptor(), 1, CONTRACT);
    let tokens = lbtc_token_source();

    let calldata = decode_hex("6bc63893000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000001e00000000000000000000000000000000000000000000000000000000000000164e288fb4a67636e97dfe9e0a5f7c0ab17636a921dc777c6aa9d6665e810ce50d61feb6d120000000000000000000000000000000000000000000000000000000000000aff00000000000000000000000089e3e4e7a699d6f131d893aeef7ee143706ac23a0000000000000000000000009ece5fb1ab62d9075c4ec814b321e24d8ea021ac000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000064155b6b130000000000000000000000008236a87084f8b84306f72007f36f2618a5634494000000000000000000000000e257c7296c7bb06db9a02fbdc8f7b3961844b556000000000000000000000000000000000000000000000000000000001878a84f00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000840000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000026000000000000000000000000000000000000000000000000000000000000002c00000000000000000000000000000000000000000000000000000000000000320000000000000000000000000000000000000000000000000000000000000038000000000000000000000000000000000000000000000000000000000000003e0000000000000000000000000000000000000000000000000000000000000044000000000000000000000000000000000000000000000000000000000000004a00000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000000000000000000000000000000056000000000000000000000000000000000000000000000000000000000000005c00000000000000000000000000000000000000000000000000000000000000620000000000000000000000000000000000000000000000000000000000000068000000000000000000000000000000000000000000000000000000000000006e0000000000000000000000000000000000000000000000000000000000000074000000000000000000000000000000000000000000000000000000000000007a0000000000000000000000000000000000000000000000000000000000000004027e2ea76c250b61c14ee188f011beb1b3edbad206b7deb75b5e379819d50813b6948cf73920719c207e9f8fe7ec7a0b8f1e77e462280915d1e8f390dfcc2cae60000000000000000000000000000000000000000000000000000000000000040a2d91fde9381925686be9b67400b9151dff595a6afe3334da598bcc518ed988618751fa323ea728b95d717cdd625f4156c72ee1b31ce7c8b82531850a9ca54a80000000000000000000000000000000000000000000000000000000000000040591868f0a526cbf110bca959fbceb2cea39437c07c0d0037169a41b9b43f3f2c6563f25f5daff79bfcbfcb5ab1a25279d7160e60eb6c8961a875e0ea58a319ec0000000000000000000000000000000000000000000000000000000000000040fbdee6aa035e91495c711cafc18ca95fe1a79844fad432f947dfc5bba686b18a171f4dc12454fd8df2808fea5bbf7b6c31142bdb7152eff3ee5f12c84692f6bb0000000000000000000000000000000000000000000000000000000000000040aa16cb15a939333e19e2dec4f278c8a4bb043885b91c27fdc273566f9a2d908b60d6cf690771c46c94904e24372e3883eeec522e61473dd662e613558ff0c37c00000000000000000000000000000000000000000000000000000000000000406a5ac271c16d71da47e7c9810df4361e155c3ff0748fb96dc6bb94882b5f68912245fde0297bdd88bcbe06d5215da2f7a7f1d928725061899a1b166e853dd69700000000000000000000000000000000000000000000000000000000000000408efad34b5f0f27c02e9ef65ff4a7428b181bab6124af6e5c6125f4da16aa638877a64f620c40ee92e3ea36092c0e3776b0852d7f61060bb1b7ad2e23f6e7719600000000000000000000000000000000000000000000000000000000000000403e672fcd3eca8b44e5663fd4b6b98dc483a540712a36bddb68105e457b3b3e4158e85705f6b73dd42900cd8962798c797ef260f9ff0806f9fef74c20488d6c2400000000000000000000000000000000000000000000000000000000000000407d1a29a911299177e4010fc1fc8427a783ccbd6c43660d67bd0d37e763d51ab14c3c476bf0efe079eba91f5528648e23ee05a0aaae36bcbb3d60d375aee6da6300000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000407cd7a9a5a102cebecbe07c68b3359d9a2fb6a851abf5cc27039ecf87807506c21dbd49da11a053eda7add7d49343e116805507d3ec35f42eb83c7d6e4ca66c8d00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000409eeaa90a8c03c1c84b81b49485145a45363d660218b631202d1afea8979496186396a2d60971435e0c6465e945bf2f3aaea5ef8bffa51ea3726bd73b9a4dc4030000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000040035b904c2633806e5552ba9a54b49c4b4fd96a39c24560270c03fd14b5a740fd3661901386c80bbd824a6527653a16a1d89f000b3e6e9097471b370ae47f883e");
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x564974801d2ffbe736ed59c9be39f6c0a4274ae6"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "Mint");
    assert_eq!(result.owner, Some("Lombard Finance".to_string()));
    // rawPayload starts with expected bytes from the on-chain data
    let payload = get_item(&result, "Payload");
    assert!(
        payload.starts_with("0xe288fb4a67636e97dfe9e0a5f7c0ab17636a921dc777c6aa9d6665e810ce50d6"),
        "payload: {payload}"
    );
    // proof is a large multi-sig proof array
    let proof = get_item(&result, "Proof");
    assert!(proof.starts_with("0x"), "proof should be hex: {proof}");
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
}
