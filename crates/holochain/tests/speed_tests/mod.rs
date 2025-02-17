//! # Speed tests
//! These are designed to diagnose performance issues from a macro level.
//! They are not intended to detect performance regressions or to be run in CI.
//! For that a latency test or benchmark should be used.
//! These tests are useful once you know there is an issue in locating which
//! part of the codebase it is.
//! An example of running the flame test to produce a flamegraph is:
//! ```fish
//! env RUST_LOG='[{}]=debug' HC_WASM_CACHE_PATH='/path/to/holochain/.wasm_cache' \
//! cargo test  --release --quiet --test speed_tests \
//! --  --nocapture --ignored --exact --test speed_test_timed_flame \
//! 2>| inferno-flamegraph > flamegraph_test_ice_(date +'%d-%m-%y-%X').svg
//! ```
//! I plan to make this all automatic as a single command in the future but it's
//! hard to automate piping from tests stderr.
//!

use std::sync::Arc;

use ::fixt::prelude::*;
use hdk::prelude::*;
use holochain::conductor::api::AdminRequest;
use holochain::conductor::api::AdminResponse;
use holochain::conductor::api::AppRequest;
use holochain::conductor::api::AppResponse;
use holochain::conductor::api::ZomeCall;
use holochain::test_utils::setup_app_in_new_conductor;
use holochain_state::nonce::fresh_nonce;
use holochain_wasm_test_utils::TestZomes;
use tempfile::TempDir;

use super::test_utils::*;
use holochain::sweettest::*;
use holochain_test_wasm_common::AnchorInput;
use holochain_trace;
use holochain_types::prelude::*;
use holochain_wasm_test_utils::TestWasm;
use holochain_websocket::WebsocketResult;
use holochain_websocket::WebsocketSender;
use matches::assert_matches;
use test_case::test_case;
use tracing::instrument;

const DEFAULT_NUM: usize = 2000;

#[tokio::test(flavor = "multi_thread")]
#[cfg(feature = "test_utils")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_prep() {
    holochain::test_utils::warm_wasm_tests();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_timed() {
    let _g = holochain_trace::test_run_timed().unwrap();
    speed_test(None).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_timed_json() {
    let _g = holochain_trace::test_run_timed_json().unwrap();
    speed_test(None).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_timed_flame() {
    let _g = holochain_trace::test_run_timed_flame(None).unwrap();
    speed_test(None).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_timed_ice() {
    let _g = holochain_trace::test_run_timed_ice(None).unwrap();
    speed_test(None).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_normal() {
    holochain_trace::test_run().unwrap();
    speed_test(None).await;
}

/// Run this test to execute the speed test, but then keep the database files
/// around in temp dirs for inspection
#[tokio::test(flavor = "multi_thread")]
#[ignore = "speed tests are ignored by default; unignore to run"]
async fn speed_test_persisted() {
    holochain_trace::test_run().unwrap();
    let envs = speed_test(None).await;
    let path = envs.path();
    println!("Run the following to see info about the test that just ran,");
    println!("with the correct cell env dir appended to the path:");
    println!();
    println!("    $ mdb_stat -afe {}/", path.to_string_lossy());
    println!();
}

#[test_case(1)]
#[test_case(10)]
#[test_case(100)]
#[test_case(1000)]
#[test_case(2000)]
#[ignore = "speed tests are ignored by default; unignore to run"]
fn speed_test_all(n: usize) {
    holochain_trace::test_run().unwrap();
    tokio_helper::block_forever_on(speed_test(Some(n)));
}

#[instrument]
async fn speed_test(n: Option<usize>) -> Arc<TempDir> {
    let num = n.unwrap_or(DEFAULT_NUM);

    // ////////////
    // START DNA
    // ////////////

    let dna_file = DnaFile::new(
        DnaDef {
            name: "need_for_speed_test".to_string(),
            modifiers: DnaModifiers {
                network_seed: "ba1d046d-ce29-4778-914b-47e6010d2faf".to_string(),
                properties: SerializedBytes::try_from(()).unwrap(),
                origin_time: Timestamp::HOLOCHAIN_EPOCH,
                quantum_time: holochain_p2p::dht::spacetime::STANDARD_QUANTUM_TIME,
            },
            integrity_zomes: vec![TestZomes::from(TestWasm::Anchor).integrity.into_inner()],
            coordinator_zomes: vec![TestZomes::from(TestWasm::Anchor).coordinator.into_inner()],
        },
        vec![TestWasm::Anchor.into()],
    )
    .await;

    // //////////
    // END DNA
    // //////////

    // ///////////
    // START ALICE
    // ///////////

    let alice_agent_id = fake_agent_pubkey_1();
    let alice_cell_id = CellId::new(dna_file.dna_hash().to_owned(), alice_agent_id.clone());
    let alice_installed_cell = InstalledCell::new(alice_cell_id.clone(), "alice_handle".into());

    // /////////
    // END ALICE
    // /////////

    // /////////
    // START BOB
    // /////////

    let bob_agent_id = fake_agent_pubkey_2();
    let bob_cell_id = CellId::new(dna_file.dna_hash().to_owned(), bob_agent_id.clone());
    let bob_installed_cell = InstalledCell::new(bob_cell_id.clone(), "bob_handle".into());

    // ///////
    // END BOB
    // ///////

    // ///////////////
    // START CONDUCTOR
    // ///////////////

    let (test_db, _app_api, handle) = setup_app_in_new_conductor(
        "test app".to_string(),
        vec![dna_file],
        vec![(alice_installed_cell, None), (bob_installed_cell, None)],
    )
    .await;

    // Setup websocket handle and app interface
    let (mut client, _) = websocket_client(&handle).await.unwrap();
    let request = AdminRequest::AttachAppInterface { port: None };
    let response = client.request(request);
    let response = response.await.unwrap();
    let app_port = match response {
        AdminResponse::AppInterfaceAttached { port } => port,
        _ => panic!("Attach app interface failed: {:?}", response),
    };
    let (mut app_interface, _) = websocket_client_by_port(app_port).await.unwrap();

    // /////////////
    // END CONDUCTOR
    // /////////////

    // ALICE DOING A CALL

    async fn new_zome_call<P>(
        cell_id: CellId,
        fn_name: FunctionName,
        payload: P,
    ) -> Result<ZomeCallUnsigned, SerializedBytesError>
    where
        P: serde::Serialize + std::fmt::Debug,
    {
        let (nonce, expires_at) = fresh_nonce(Timestamp::now()).unwrap();
        Ok(ZomeCallUnsigned {
            cell_id: cell_id.clone(),
            zome_name: TestWasm::Anchor.into(),
            cap_secret: Some(CapSecretFixturator::new(Unpredictable).next().unwrap()),
            fn_name,
            payload: ExternIO::encode(payload)?,
            provenance: cell_id.agent_pubkey().clone(),
            nonce,
            expires_at,
        })
    }

    let anchor_invocation = |anchor: String, cell_id, i: usize| async move {
        let anchor = AnchorInput(anchor.clone(), i.to_string());
        new_zome_call(cell_id, "anchor".into(), anchor).await
    };

    async fn call(
        app_interface: &mut WebsocketSender,
        invocation: ZomeCall,
    ) -> WebsocketResult<AppResponse> {
        let request = AppRequest::CallZome(Box::new(invocation));
        app_interface.request(request).await
    }

    let timer = std::time::Instant::now();

    for i in 0..num {
        let invocation = anchor_invocation("alice".to_string(), alice_cell_id.clone(), i)
            .await
            .unwrap();
        let response = call(
            &mut app_interface,
            ZomeCall::try_from_unsigned_zome_call(handle.keystore(), invocation)
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_matches!(response, AppResponse::ZomeCalled(_));
        let invocation = anchor_invocation("bobbo".to_string(), bob_cell_id.clone(), i)
            .await
            .unwrap();
        let response = call(
            &mut app_interface,
            ZomeCall::try_from_unsigned_zome_call(handle.keystore(), invocation)
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_matches!(response, AppResponse::ZomeCalled(_));
    }

    let mut alice_done = false;
    let mut bobbo_done = false;
    let mut alice_attempts = 0_usize;
    let mut bobbo_attempts = 0_usize;
    loop {
        if !bobbo_done {
            bobbo_attempts += 1;
            let invocation = new_zome_call(
                alice_cell_id.clone(),
                "list_anchor_addresses".into(),
                "bobbo".to_string(),
            )
            .await
            .unwrap();
            let response = call(
                &mut app_interface,
                ZomeCall::try_from_unsigned_zome_call(handle.keystore(), invocation)
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
            let hashes: EntryHashes = match response {
                AppResponse::ZomeCalled(r) => r.decode().unwrap(),
                _ => unreachable!(),
            };
            bobbo_done = hashes.0.len() == num;
        }

        if !alice_done {
            alice_attempts += 1;
            let invocation = new_zome_call(
                bob_cell_id.clone(),
                "list_anchor_addresses".into(),
                "alice".to_string(),
            )
            .await
            .unwrap();
            let response = call(
                &mut app_interface,
                ZomeCall::try_from_unsigned_zome_call(handle.keystore(), invocation)
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
            let hashes: EntryHashes = match response {
                AppResponse::ZomeCalled(r) => r.decode().unwrap(),
                _ => unreachable!(),
            };
            alice_done = hashes.0.len() == num;
        }
        if alice_done && bobbo_done {
            let el = timer.elapsed();
            println!(
                "Consistency in for {} calls: {}ms or {}s\n
                Alice took {} attempts to reach consistency\n
                Bobbo took {} attempts to reach consistency",
                num,
                el.as_millis(),
                el.as_secs(),
                alice_attempts,
                bobbo_attempts,
            );
            break;
        }
    }

    handle.shutdown().await.unwrap().unwrap();
    test_db
}
