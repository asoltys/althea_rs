//! This is the main source file for the Rita client binary, by 'client' we mean 'not an exit server'
//! all meshing and billing functionaltiy is contained in rita_common and is common to both rita and
//! rita_exit. The major difference is billing and connection code for the 'exit', the mandatory
//! vpn system integrated into the Althea network design, as well as API endpoints for a management
//! dashboard of router functions like wifi, which the exit is not expected to have.
//!
//! This file initilizes the dashboard endpoints for the client as well as the common and client
//! specific actors.

#![cfg_attr(
    feature = "system_alloc",
    feature(alloc_system, allocator_api)
)]
#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]

#[cfg(feature = "system_alloc")]
extern crate alloc_system;

#[cfg(feature = "system_alloc")]
use alloc_system::System;

#[cfg(feature = "system_alloc")]
#[global_allocator]
static A: System = System;

#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

extern crate actix;
extern crate actix_web;
extern crate byteorder;
extern crate bytes;
extern crate clu;
extern crate docopt;
extern crate env_logger;
extern crate eui48;
extern crate futures;
extern crate handlebars;
extern crate ipnetwork;
extern crate lettre;
extern crate lettre_email;
extern crate minihttpse;
extern crate num_traits;
extern crate openssl_probe;
extern crate rand;
extern crate regex;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
extern crate settings;
extern crate syslog;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;
extern crate trust_dns_resolver;

use docopt::Docopt;
#[cfg(not(test))]
use settings::FileWrite;

use settings::{RitaClientSettings, RitaCommonSettings, RitaSettingsStruct};

use actix::registry::SystemService;
use actix::*;
use actix_web::http::Method;
use actix_web::*;

use std::sync::{Arc, RwLock};

#[cfg(test)]
use std::sync::Mutex;

extern crate althea_kernel_interface;
extern crate althea_types;
extern crate babel_monitor;
extern crate num256;

pub mod actix_utils;
mod middleware;
mod rita_client;
mod rita_common;

use rita_client::dashboard::network_endpoints::*;
use rita_common::dashboard::network_endpoints::*;
use rita_common::network_endpoints::*;

#[derive(Debug, Deserialize)]
struct Args {
    flag_config: String,
    flag_platform: String,
    flag_future: bool,
}

lazy_static! {
    static ref USAGE: String = format!(
        "Usage: rita --config=<settings> --platform=<platform> [--future]
Options:
    -c, --config=<settings>     Name of config file
    -p, --platform=<platform>   Platform (linux or openwrt)
    --future                    Enable B side of A/B releases
About:
    Version {}
    git hash {}",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH")
    );
}

use althea_kernel_interface::KernelInterface;

#[cfg(not(test))]
use althea_kernel_interface::LinuxCommandRunner;
#[cfg(test)]
use althea_kernel_interface::TestCommandRunner;

#[cfg(test)]
lazy_static! {
    pub static ref KI: Box<KernelInterface> = Box::new(TestCommandRunner {
        run_command: Arc::new(Mutex::new(Box::new(|_program, _args| {
            panic!("kernel interface used before initialized");
        })))
    });
}

#[cfg(not(test))]
lazy_static! {
    pub static ref KI: Box<KernelInterface> = Box::new(LinuxCommandRunner {});
}

#[cfg(not(test))]
lazy_static! {
    pub static ref SETTING: Arc<RwLock<RitaSettingsStruct>> = {
        let args: Args = Docopt::new((*USAGE).as_str())
            .and_then(|d| d.deserialize())
            .unwrap_or_else(|e| e.exit());

        let settings_file = args.flag_config;
        let platform = args.flag_platform;

        let s = RitaSettingsStruct::new_watched(&settings_file).unwrap();

        s.set_future(args.flag_future);

        clu::init(&platform, s.clone());

        s.read().unwrap().write(&settings_file).unwrap();
        s
    };
}

#[cfg(test)]
lazy_static! {
    pub static ref SETTING: Arc<RwLock<RitaSettingsStruct>> =
        { Arc::new(RwLock::new(RitaSettingsStruct::default())) };
}

fn main() {
    // On Linux static builds we need to probe ssl certs path to be able to
    // do TLS stuff.
    openssl_probe::init_ssl_cert_env_vars();

    if !SETTING.get_log().enabled {
        env_logger::init();
    }

    if cfg!(feature = "development") {
        println!("Warning!");
        println!("This build is meant only for development purposes.");
        println!("Running this on production is unsupported and not safe!");
    }

    let args: Args = Docopt::new((*USAGE).as_str())
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());

    let settings_file = args.flag_config;

    // to get errors before lazy static
    RitaSettingsStruct::new(&settings_file).expect("Settings parse failure");

    trace!("Starting");
    info!(
        "crate ver {}, git hash {}",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH")
    );
    trace!("Starting with Identity: {:?}", SETTING.get_identity());

    let system = actix::System::new(format!("main {:?}", SETTING.get_network().mesh_ip));

    assert!(rita_common::debt_keeper::DebtKeeper::from_registry().connected());
    assert!(rita_common::payment_controller::PaymentController::from_registry().connected());
    assert!(rita_common::tunnel_manager::TunnelManager::from_registry().connected());
    assert!(rita_common::http_client::HTTPClient::from_registry().connected());
    assert!(rita_common::traffic_watcher::TrafficWatcher::from_registry().connected());
    assert!(rita_common::peer_listener::PeerListener::from_registry().connected());
    assert!(rita_client::exit_manager::ExitManager::from_registry().connected());

    // rita
    server::new(|| App::new().resource("/hello", |r| r.method(Method::POST).with(hello_response)))
        .workers(1)
        .bind(format!("[::0]:{}", SETTING.get_network().rita_hello_port))
        .unwrap()
        .shutdown_timeout(0)
        .start();
    server::new(|| {
        App::new().resource("/make_payment", |r| {
            r.method(Method::POST).with(make_payments)
        })
    }).workers(1)
    .bind(format!("[::0]:{}", SETTING.get_network().rita_contact_port))
    .unwrap()
    .shutdown_timeout(0)
    .start();

    // dashboard
    server::new(|| {
        App::new()
            .middleware(middleware::Headers)
            .route("/dao_list", Method::GET, get_dao_list)
            .route("/dao_list/add/{address}", Method::POST, add_to_dao_list)
            .route(
                "/dao_list/remove/{address}",
                Method::POST,
                remove_from_dao_list,
            ).route("/debts", Method::GET, get_debts)
            .route("/exits", Method::GET, get_exit_info)
            .route("/exits/{name}/register", Method::POST, register_to_exit)
            .route("/exits/{name}/reset", Method::POST, reset_exit)
            .route("/exits/{name}/select", Method::POST, select_exit)
            .route(
                "/exits/{name}/verify/{code}",
                Method::POST,
                verify_on_exit_with_code,
            ).route(
                "/remote_logging/enabled/{enabled}",
                Method::POST,
                remote_logging,
            ).route(
                "/remote_logging/level/{level}",
                Method::POST,
                remote_logging_level,
            ).route("/info", Method::GET, get_own_info)
            .route("/interfaces", Method::GET, get_interfaces)
            .route("/interfaces", Method::POST, set_interfaces)
            .route("/mesh_ip", Method::GET, get_mesh_ip)
            .route("/mesh_ip", Method::POST, set_mesh_ip)
            .route("/neighbors", Method::GET, get_node_info)
            .route("/settings", Method::GET, get_settings)
            .route("/settings", Method::POST, set_settings)
            .route("/version", Method::GET, version)
            .route("/wifi_settings/pass", Method::POST, set_wifi_pass)
            .route("/wifi_settings/ssid", Method::POST, set_wifi_ssid)
            .route("/wifi_settings", Method::GET, get_wifi_config)
            .route("/wipe", Method::POST, wipe)
    }).workers(1)
    .bind(format!(
        "[::0]:{}",
        SETTING.get_network().rita_dashboard_port
    )).unwrap()
    .shutdown_timeout(0)
    .start();

    let common = rita_common::rita_loop::RitaLoop::new();
    let _: Addr<_> = common.start();

    let client = rita_client::rita_loop::RitaLoop {};
    let _: Addr<_> = client.start();

    system.run();
}
