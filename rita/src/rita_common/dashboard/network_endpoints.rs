use actix::registry::SystemService;
use rita_common::debt_keeper::GetDebtsList;

use althea_types::EthAddress;

use futures::Future;

use std::boxed::Box;

use failure::Error;

use serde_json;

use settings::RitaCommonSettings;
use SETTING;

use super::{Dashboard, GetOwnInfo, OwnInfo};
use actix_web::*;

use rita_common::debt_keeper::{DebtKeeper, GetDebtsResult};
use rita_common::network_endpoints::JsonStatusResponse;

pub fn get_own_info(_req: HttpRequest) -> Box<Future<Item = Json<OwnInfo>, Error = Error>> {
    debug!("Get own info endpoint hit!");
    Dashboard::from_registry()
        .send(GetOwnInfo {})
        .from_err()
        .and_then(move |reply| Ok(Json(reply?)))
        .responder()
}

pub fn get_settings(_req: HttpRequest) -> Result<Json<serde_json::Value>, Error> {
    debug!("Get settings endpoint hit!");
    Ok(Json(SETTING.get_all()?))
}

pub fn set_settings(
    new_settings: Json<serde_json::Value>,
) -> Result<Json<JsonStatusResponse>, Error> {
    debug!("Set settings endpoint hit!");
    SETTING.merge(new_settings.into_inner())?;

    JsonStatusResponse::new(Ok("New settings applied".to_string()))
}

#[cfg(not(feature = "development"))]
pub fn wipe(_req: HttpRequest) -> Result<HttpResponse, Error> {
    // This is returned on production builds.
    Ok(HttpResponse::NotFound().finish())
}

#[cfg(feature = "development")]
pub fn wipe(_req: HttpRequest) -> Result<HttpResponse, Error> {
    // Clean up existing WG interfaces
    match cleanup() {
        Ok(_) => trace!("wipe: WireGuard interfaces cleanup success!"),
        Err(e) => {
            warn!(
                "wipe: Unable to complete WireGuard interfaces cleanup: {:?}",
                e
            );
            return Err(e);
        }
    }

    // Restore default route
    match KI.restore_default_route(&mut SETTING.get_network_mut().default_route) {
        Ok(_) => trace!("wipe: Restore default route success!"),
        Err(e) => {
            warn!("wipe: Unable to restore default route: {:?}", e);
            return Err(e);
        }
    }

    // Create new WireGuard keys
    match linux_generate_wg_keys(&mut SETTING.get_network_mut()) {
        Ok(_) => trace!("wipe: Generated new WireGuard keys"),
        Err(e) => {
            warn!("wipe: Unable to generate new WireGuard keys: {:?}", e);
            return Err(e);
        }
    }
    // Generate new mesh IP
    match linux_generate_mesh_ip(&mut SETTING.get_network_mut()) {
        Ok(_) => trace!("wipe: Generated new mesh IP"),
        Err(e) => {
            warn!("wipe: Unable to generate new mesh IP: {:?}", e);
            return Err(e);
        }
    }

    // Creates file on disk containing key
    match KI.create_wg_key(
        &Path::new(&SETTING.get_network().wg_private_key_path),
        &SETTING.get_network().wg_private_key,
    ) {
        Ok(_) => trace!("wipe: Saved new WireGuard keys to disk"),
        Err(e) => {
            warn!("wipe: Unable to save new WireGuard keys: {:?}", e);
            return Err(e);
        }
    }

    Ok(HttpResponse::NoContent().finish())
}

pub fn get_debts(
    _req: HttpRequest,
) -> Box<Future<Item = Json<Vec<GetDebtsResult>>, Error = Error>> {
    trace!("get_debts: Hit");
    DebtKeeper::from_registry()
        .send(GetDebtsList {})
        .from_err()
        .and_then(move |reply| Ok(Json(reply?)))
        .responder()
}

pub fn get_dao_list(_req: HttpRequest) -> Result<Json<Vec<EthAddress>>, Error> {
    trace!("get dao list: Hit");
    Ok(Json(SETTING.get_dao().dao_addresses.clone()))
}

pub fn add_to_dao_list(path: Path<(EthAddress)>) -> Result<Json<()>, Error> {
    trace!("Add to dao list: Hit");
    let provided_address = path.into_inner();
    for address in SETTING.get_dao().dao_addresses.iter() {
        if *address == provided_address {
            return Ok(Json(()));
        }
    }
    SETTING.get_dao_mut().dao_addresses.push(provided_address);
    Ok(Json(()))
}

pub fn remove_from_dao_list(path: Path<(EthAddress)>) -> Result<Json<()>, Error> {
    trace!("Remove from dao list: Hit");
    let provided_address = path.into_inner();
    let mut iter = 0;
    let mut found = false;
    for address in SETTING.get_dao().dao_addresses.iter() {
        if *address == provided_address {
            found = true;
            break;
        }
        iter = iter + 1;
    }
    if found {
        SETTING.get_dao_mut().dao_addresses.remove(iter);
    }
    Ok(Json(()))
}
