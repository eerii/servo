/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Liberally derived from the [Firefox JS implementation](http://mxr.mozilla.org/mozilla-central/source/toolkit/devtools/server/actors/webconsole.js).
//! Handles interaction with the remote web console on network events (HTTP requests, responses) in Servo.

use std::cell::RefCell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Local, LocalResult, TimeZone};
use devtools_traits::{HttpRequest, HttpResponse};
use headers::{ContentLength, ContentType, Cookie, HeaderMapExt};
use http::{HeaderMap, Method};
use net::cookie::ServoCookie;
use net_traits::http_status::HttpStatus;
use net_traits::request::{Destination as RequestDestination, RequestHeadersSize};
use net_traits::{CookieSource, TlsSecurityInfo, TlsSecurityState};
use serde::Serialize;
use serde_json::{Map, Value};
use servo_url::ServoUrl;

use crate::StreamId;
use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::actors::long_string::LongStringActor;
use crate::network_handler::Cause;
use crate::protocol::ClientRequest;

struct ParsedHeaders {
    headers: Vec<Header>,
    size: usize,
    raw: String,
}

impl ParsedHeaders {
    fn from(map: &HeaderMap) -> Self {
        let mut headers = vec![];
        let mut size = 0;
        let mut raw = "".to_owned();

        for (name, value) in map {
            let name = name.as_str().to_owned();
            let value = value.to_str().unwrap().to_owned();
            size += name.len() + value.len();
            raw = raw + &name + ":" + &value + "\r\n";
            headers.push(Header { name, value });
        }

        Self { headers, size, raw }
    }
}

pub struct DevtoolsHttpRequest {
    pub url: String,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub started: SystemTime,
    pub time_stamp: i64,
    pub destination: RequestDestination,
    pub cookies: Vec<DevtoolsCookie>,
    pub is_xhr: bool,
}

impl From<HttpRequest> for DevtoolsHttpRequest {
    fn from(req: HttpRequest) -> Self {
        let cookies = req
            .headers
            .typed_get::<Cookie>()
            .map(|headers| {
                headers
                    .iter()
                    .map(|cookie| DevtoolsCookie {
                        name: cookie.0.to_string(),
                        value: cookie.1.to_string(),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            url: req.url.to_string(),
            method: req.method,
            headers: req.headers,
            body: req.body.as_ref().map(|body| body.0.clone()),
            started: req.started_date_time,
            time_stamp: req.time_stamp,
            destination: req.destination,
            cookies,
            is_xhr: req.is_xhr,
        }
    }
}

pub struct DevtoolsHttpResponse {
    headers: Option<HeaderMap>,
    body: Option<Vec<u8>>,
    status: HttpStatus,
    cookies: Vec<DevtoolsCookie>,
}

impl DevtoolsHttpResponse {
    fn content(&self) -> Content {
        let mime_type = self
            .headers
            .as_ref()
            .and_then(|h| h.typed_get::<ContentType>())
            .map(|ct| ct.to_string())
            .unwrap_or_default();
        let transferred_size = self
            .headers
            .as_ref()
            .and_then(|hdrs| hdrs.typed_get::<ContentLength>())
            .map(|cl| cl.0);
        let content_size = self.body.as_ref().map(|body| body.len() as u64);
        Content {
            mime_type,
            content_size: content_size.unwrap_or(0) as u32,
            transferred_size: transferred_size.unwrap_or(0) as u32,
            discard_response_body: false,
        }
    }
}

impl From<HttpResponse> for DevtoolsHttpResponse {
    fn from(res: HttpResponse) -> Self {
        let body = res.body.as_ref().map(|body| body.0.clone());

        // TODO: URL
        let cookies = (|| {
            let headers = res.headers.as_ref()?;
            let url = ServoUrl::parse("https://servo.org").ok()?;
            let cookies = headers
                .get_all("set-cookie")
                .iter()
                .filter_map(|cookie| {
                    let cookie_str = std::str::from_utf8(cookie.as_bytes()).ok()?;
                    ServoCookie::from_cookie_string(cookie_str, &url, CookieSource::HTTP)
                })
                .map(|servo_cookie| {
                    let c = &servo_cookie.cookie;
                    DevtoolsCookie {
                        name: c.name().to_string(),
                        value: c.value().to_string(),
                        path: c.path().map(|p| p.to_string()),
                        domain: c.domain().map(|d| d.to_string()),
                        expires: c.expires().map(|dt| format!("{:?}", dt)),
                        http_only: c.http_only(),
                        secure: c.secure(),
                        same_site: c.same_site().map(|s| s.to_string()),
                    }
                })
                .collect::<Vec<_>>();
            Some(cookies)
        })()
        .unwrap_or_default();

        Self {
            headers: res.headers,
            body,
            status: res.status,
            cookies,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheDetails {
    from_cache: bool,
    from_service_worker: bool,
}

#[derive(Serialize)]
struct Header {
    name: String,
    value: String,
}

#[derive(Clone, Default, Serialize)]
pub struct Timings {
    blocked: u32,
    dns: u32,
    connect: u64,
    send: u64,
    wait: u32,
    receive: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseContent {
    mime_type: String,
    text: Value,
    body_size: usize,
    decoded_body_size: usize,
    size: usize,
    headers_size: usize,
    transferred_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct CertificateIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    common_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organizational_unit: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct CertificateValidity {
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifetime: Option<String>,
    expired: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct CertificateFingerprint {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SecurityCertificate {
    subject: CertificateIdentity,
    issuer: CertificateIdentity,
    validity: CertificateValidity,
    fingerprint: CertificateFingerprint,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_built_in_root: Option<bool>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SecurityInfo {
    state: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    weakness_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cipher_suite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kea_group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_scheme_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alpn_protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    certificate_transparency: Option<String>,
    hsts: bool,
    hpkp: bool,
    used_ech: bool,
    used_delegated_credentials: bool,
    used_ocsp: bool,
    used_private_dns: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    certificate_chain: Vec<String>,
    cert: SecurityCertificate,
}

impl From<&TlsSecurityInfo> for SecurityInfo {
    fn from(info: &TlsSecurityInfo) -> Self {
        Self {
            state: info.state.to_string(),
            weakness_reasons: info.weakness_reasons.clone(),
            protocol_version: info.protocol_version.clone(),
            cipher_suite: info.cipher_suite.clone(),
            kea_group_name: info.kea_group_name.clone(),
            signature_scheme_name: info.signature_scheme_name.clone(),
            alpn_protocol: info.alpn_protocol.clone(),
            certificate_transparency: info
                .certificate_transparency
                .clone()
                .or_else(|| Some("unknown".to_string())),
            hsts: info.hsts,
            hpkp: info.hpkp,
            used_ech: info.used_ech,
            used_delegated_credentials: info.used_delegated_credentials,
            used_ocsp: info.used_ocsp,
            used_private_dns: info.used_private_dns,
            ..Default::default()
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetRequestHeadersReply {
    from: String,
    headers: Vec<Header>,
    header_size: usize,
    raw_headers: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetResponseHeadersReply {
    from: String,
    headers: Vec<Header>,
    header_size: usize,
    raw_headers: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetResponseContentReply {
    from: String,
    content: Option<ResponseContent>,
    content_discarded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetRequestPostDataReply {
    from: String,
    post_data: Option<Vec<u8>>,
    post_data_discarded: bool,
}

#[derive(Serialize)]
struct GetCookiesReply {
    from: String,
    cookies: Vec<DevtoolsCookie>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetEventTimingsReply {
    from: String,
    timings: Timings,
    total_time: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetSecurityInfoReply {
    from: String,
    security_info: SecurityInfo,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEventMsg {
    pub actor: String,
    pub resource_id: u64,
    pub url: String,
    pub method: String,
    pub started_date_time: String,
    pub time_stamp: i64,
    #[serde(rename = "isXHR")]
    pub is_xhr: bool,
    pub private: bool,
    pub cause: Cause,
}

#[derive(Default)]
pub struct NetworkEventActor {
    pub name: String,
    pub cache_details: RefCell<Option<CacheDetails>>,
    pub event_timing: RefCell<Option<Timings>>,
    pub request: RefCell<Option<DevtoolsHttpRequest>>,
    pub resource_id: u64,
    pub response: RefCell<Option<DevtoolsHttpResponse>>,
    pub security_info: RefCell<Option<TlsSecurityInfo>>,
    pub security_state: RefCell<String>,
    pub total_time: RefCell<Duration>,
    pub watcher_name: String,
}

impl Actor for NetworkEventActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn handle_message(
        &self,
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "getRequestHeaders" => {
                let req = self.request.borrow();
                let req = req.as_ref().ok_or(ActorError::Internal)?;
                let headers = ParsedHeaders::from(&req.headers);

                let msg = GetRequestHeadersReply {
                    from: self.name(),
                    headers: headers.headers,
                    header_size: headers.size,
                    raw_headers: headers.raw,
                };
                request.reply_final(&msg)?
            },

            "getRequestCookies" => {
                let req = self.request.borrow();
                let req = req.as_ref().ok_or(ActorError::Internal)?;

                let msg = GetCookiesReply {
                    from: self.name(),
                    cookies: req.cookies.clone(),
                };

                request.reply_final(&msg)?
            },

            "getRequestPostData" => {
                let req = self.request.borrow();
                let req = req.as_ref().ok_or(ActorError::Internal)?;

                let msg = GetRequestPostDataReply {
                    from: self.name(),
                    post_data: req.body.clone(),
                    post_data_discarded: req.body.is_none(),
                };
                request.reply_final(&msg)?
            },

            "getResponseHeaders" => {
                let res = self.response.borrow();
                let res = res.as_ref().ok_or(ActorError::Internal)?;
                // FIXME: what happens when there are no response headers?
                let headers =
                    ParsedHeaders::from(res.headers.as_ref().ok_or(ActorError::Internal)?);

                let msg = GetResponseHeadersReply {
                    from: self.name(),
                    headers: headers.headers,
                    header_size: headers.size,
                    raw_headers: headers.raw,
                };
                request.reply_final(&msg)?;
            },

            "getResponseCookies" => {
                let res = self.response.borrow();
                let res = res.as_ref().ok_or(ActorError::Internal)?;

                let msg = GetCookiesReply {
                    from: self.name(),
                    cookies: res.cookies.clone(),
                };
                request.reply_final(&msg)?
            },

            "getResponseContent" => {
                let res = self.response.borrow();
                let res = res.as_ref().ok_or(ActorError::Internal)?;
                let content = res.content();
                let headers =
                    ParsedHeaders::from(res.headers.as_ref().ok_or(ActorError::Internal)?);

                let content_obj = res.body.as_ref().map(|body| {
                    let body_size = body.len();
                    let decoded_body_size = body.len();
                    let size = body.len();

                    if Self::is_text_mime(&content.mime_type) {
                        let full_str = String::from_utf8_lossy(body).to_string();

                        // Queue a LongStringActor for this body
                        let long_string_actor = LongStringActor::new(registry, full_str);
                        let long_string_obj = long_string_actor.long_string_obj();
                        registry.register_later(long_string_actor);

                        ResponseContent {
                            mime_type: content.mime_type,
                            text: serde_json::to_value(long_string_obj).unwrap(),
                            body_size,
                            decoded_body_size,
                            size,
                            headers_size: headers.size,
                            transferred_size: content.transferred_size as usize,
                            encoding: None,
                        }
                    } else {
                        let b64 = STANDARD.encode(body);
                        ResponseContent {
                            mime_type: content.mime_type,
                            text: serde_json::to_value(b64).unwrap(),
                            body_size,
                            decoded_body_size,
                            size,
                            headers_size: headers.size,
                            transferred_size: content.transferred_size as usize,
                            encoding: Some("base64".to_string()),
                        }
                    }
                });
                let msg = GetResponseContentReply {
                    from: self.name(),
                    content: content_obj,
                    content_discarded: res.body.is_none(),
                };
                request.reply_final(&msg)?
            },

            "getEventTimings" => {
                // TODO: This is a fake timings msg
                let timings_obj = self.event_timing.borrow().clone().unwrap_or_default();
                // Might use the one on self
                let total = timings_obj.connect + timings_obj.send;
                // TODO: Send the correct values for all these fields.
                let msg = GetEventTimingsReply {
                    from: self.name(),
                    timings: timings_obj,
                    total_time: total,
                };
                request.reply_final(&msg)?
            },

            "getSecurityInfo" => {
                let security_info = self.security_info.borrow();
                let msg = GetSecurityInfoReply {
                    from: self.name(),
                    security_info: security_info.as_ref().map(From::from).unwrap_or_else(|| {
                        SecurityInfo {
                            state: self.security_state.borrow().clone(),
                            ..Default::default()
                        }
                    }),
                };
                request.reply_final(&msg)?
            },

            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl NetworkEventActor {
    pub fn new(name: String, resource_id: u64, watcher_name: String) -> NetworkEventActor {
        NetworkEventActor {
            name,
            resource_id,
            security_state: RefCell::from("insecure".to_owned()),
            watcher_name,
            ..Default::default()
        }
    }

    pub fn set_request(&self, request: HttpRequest) {
        self.total_time
            .replace(request.connect_time + request.send_time);
        self.event_timing.replace(Some(Timings {
            connect: request.connect_time.as_millis() as u64,
            send: request.send_time.as_millis() as u64,
            ..Default::default()
        }));
        self.request.replace(Some(request.into()));
    }

    pub fn set_response(&self, response: HttpResponse) {
        self.cache_details.replace(Some(CacheDetails {
            from_cache: response.from_cache,
            from_service_worker: false,
        }));
        self.response.replace(Some(response.into()));
    }

    pub fn set_security_info(&self, security_info: Option<TlsSecurityInfo>) {
        self.security_state.replace(
            security_info
                .as_ref()
                .map(|info| info.state)
                .unwrap_or(TlsSecurityState::Insecure)
                .to_string(),
        );
        self.security_info.replace(security_info);
    }

    pub fn resource_updates(&self) -> NetworkEventResource {
        let req = self.request.borrow();
        let res = self.response.borrow();
        // TODO: Review all of this fields, if they should be here
        // TODO: Merge header number and size
        // TODO: Set the correct values for these fields
        NetworkEventResource {
            resource_id: self.resource_id,
            resource_updates: ResourceUpdates {
                request: req.as_ref().map(|r| r.into()),
                response: res.as_ref().map(|r| r.into()),
                total_time: self.total_time.borrow().as_secs_f64(),
                security_state: self.security_state.borrow().clone(),
                security_info_available: self.security_info.borrow().is_some(),
                event_timings_available: self.event_timing.borrow().is_some(),
            },
            browsing_context_id: 0,
            inner_window_id: 0,
        }
    }

    fn is_text_mime(mime: &str) -> bool {
        let lower = mime.to_ascii_lowercase();
        lower.starts_with("text/") ||
            lower.contains("json") ||
            lower.contains("javascript") ||
            lower.contains("xml") ||
            lower.contains("csv") ||
            lower.contains("html")
    }
}

impl ActorEncode<NetworkEventMsg> for NetworkEventActor {
    fn encode(&self, _: &ActorRegistry) -> NetworkEventMsg {
        let request = self.request.borrow();
        let req = request.as_ref().expect("A request should have been set");

        let started_datetime_rfc3339 = match Local.timestamp_millis_opt(
            req.started
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        ) {
            LocalResult::None => "".to_owned(),
            LocalResult::Single(date_time) => date_time.to_rfc3339().to_string(),
            LocalResult::Ambiguous(date_time, _) => date_time.to_rfc3339().to_string(),
        };

        // TODO: Send the correct values for startedDateTime, isXHR, private
        NetworkEventMsg {
            actor: self.name(),
            resource_id: self.resource_id,
            url: req.url.clone(),
            method: format!("{}", req.method),
            started_date_time: started_datetime_rfc3339,
            time_stamp: req.time_stamp,
            is_xhr: req.is_xhr,
            private: false,
            cause: Cause {
                type_: req.destination.as_str().to_string(),
                loading_document_uri: None, // Set if available
            },
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct DevtoolsCookie {
    name: String,
    value: String,
    // Only for responses, not for requests
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_site: Option<String>,
}

#[derive(Clone, Serialize)]
struct Cookies {
    cookies: Vec<DevtoolsCookie>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Headers {
    headers: usize,
    headers_size: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    http_version: String,
    remote_address: String,
    remote_port: u32,
    status: String,
    status_text: String,
    discard_response_body: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Content {
    mime_type: String,
    content_size: u32,
    transferred_size: u32,
    discard_response_body: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevtoolsHttpRequestMsg {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    request_headers: Option<Headers>,
    request_headers_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    request_cookies: Option<Cookies>,
    request_cookies_available: bool,
}

impl From<&DevtoolsHttpRequest> for DevtoolsHttpRequestMsg {
    fn from(req: &DevtoolsHttpRequest) -> Self {
        let request_headers_available = !req.headers.is_empty();
        let request_cookies_available = !req.cookies.is_empty();
        Self {
            request_headers: request_headers_available.then_some(Headers {
                headers: req.headers.len(),
                headers_size: req.headers.total_size(),
            }),
            request_headers_available,
            request_cookies: request_cookies_available.then_some(Cookies {
                cookies: req.cookies.clone(),
            }),
            request_cookies_available,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevtoolsHttpResponseMsg {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    response_headers: Option<Headers>,
    response_headers_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    response_cookies: Option<Cookies>,
    response_cookies_available: bool,
    #[serde(flatten)]
    response_start: Start,
    response_start_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    response_content: Option<Content>,
    response_content_available: bool,
}

impl From<&DevtoolsHttpResponse> for DevtoolsHttpResponseMsg {
    fn from(res: &DevtoolsHttpResponse) -> Self {
        let response_headers = res.headers.as_ref().map(|headers| {
            let parsed = ParsedHeaders::from(headers);
            Headers {
                headers: parsed.headers.len(),
                headers_size: parsed.size,
            }
        });

        let response_cookies_available = !res.cookies.is_empty();

        // TODO: Send the correct values for all these fields.
        let response_start = Start {
            http_version: "HTTP/1.1".to_owned(),
            remote_address: "63.245.217.43".to_owned(),
            remote_port: 443,
            status: res.status.code().to_string(),
            status_text: String::from_utf8_lossy(res.status.message()).to_string(),
            discard_response_body: false,
        };

        let content = res.content();
        let response_content = (content.content_size > 0).then_some(content);
        let response_content_available = response_content.is_some();

        Self {
            response_headers,
            response_headers_available: res.headers.is_some(),
            response_cookies: response_cookies_available.then_some(Cookies {
                cookies: res.cookies.clone(),
            }),
            response_cookies_available,
            response_start,
            response_start_available: true,
            response_content,
            response_content_available,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUpdates {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    request: Option<DevtoolsHttpRequestMsg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    response: Option<DevtoolsHttpResponseMsg>,
    total_time: f64,
    security_state: String,
    security_info_available: bool,
    event_timings_available: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEventResource {
    pub resource_id: u64,
    pub resource_updates: ResourceUpdates,
    pub browsing_context_id: u64,
    pub inner_window_id: u64,
}
