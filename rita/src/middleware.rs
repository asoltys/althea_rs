//! This is the Actix-web middleware that attaches the content headers we need for
//! the client dashboard

use crate::http::{header, HttpTryFrom, Method, StatusCode};
use crate::SETTING;
use actix_web::middleware::{Middleware, Response, Started};
use actix_web::{FromRequest, HttpRequest, HttpResponse, Result};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use actix_web_httpauth::extractors::AuthenticationError;
use regex::Regex;
use settings::RitaCommonSettings;

pub struct Headers;

impl<S> Middleware<S> for Headers {
    fn start(&self, _req: &HttpRequest<S>) -> Result<Started> {
        Ok(Started::Done)
    }

    fn response(&self, req: &HttpRequest<S>, mut resp: HttpResponse) -> Result<Response> {
        let url = req.connection_info().host().to_owned();
        let re = Regex::new(r"^(.*):").unwrap();
        let url_no_port = re.captures(&url).unwrap()[1].to_string();
        if req.method() == Method::OPTIONS {
            *resp.status_mut() = StatusCode::OK;
        }
        resp.headers_mut().insert(
            header::HeaderName::try_from("Access-Control-Allow-Origin").unwrap(),
            header::HeaderValue::from_str(&format!("http://{}", url_no_port)).unwrap(),
        );
        resp.headers_mut().insert(
            header::HeaderName::try_from("Access-Control-Allow-Headers").unwrap(),
            header::HeaderValue::from_static("content-type"),
        );
        Ok(Response::Done(resp))
    }
}

pub struct Auth;

impl<S> Middleware<S> for Auth {
    fn start(&self, req: &HttpRequest<S>) -> Result<Started> {
        let password = SETTING.get_network().rita_dashboard_password.clone();
        let mut config = Config::default();
        config.realm("Admin");
        let auth = BasicAuth::from_request(&req, &config)?;

        if password.is_none() {
            Ok(Started::Done)
        // hardcoded username since we don't have a user system
        } else if auth.username() == "rita"
            && auth.password().is_some()
            && auth.password().unwrap() == password.unwrap()
        {
            Ok(Started::Done)
        } else {
            Err(AuthenticationError::from(config).into())
        }
    }
}
