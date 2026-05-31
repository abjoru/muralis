//! Test support: a canned [`HttpFetch`] that records calls and replays
//! fixed responses. Available to downstream source crates for their tests.

use std::sync::Mutex;

use async_trait::async_trait;
use reqwest::StatusCode;

use muralis_core::error::Result;

use crate::HttpFetch;

/// One captured request.
#[derive(Clone, Debug)]
pub struct RecordedCall {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
}

impl RecordedCall {
    /// Value of the first query param matching `key`, if any.
    pub fn query_value(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Value of the first header matching `name`, if any.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Replays `responses` in order (clamping to the last once exhausted) and
/// records every call for assertions.
pub struct StubFetch {
    responses: Vec<(StatusCode, bytes::Bytes)>,
    calls: Mutex<Vec<RecordedCall>>,
    next: Mutex<usize>,
}

impl StubFetch {
    pub fn new(responses: Vec<(StatusCode, bytes::Bytes)>) -> Self {
        Self {
            responses,
            calls: Mutex::new(Vec::new()),
            next: Mutex::new(0),
        }
    }

    /// A single `200 OK` JSON body.
    pub fn ok(body: &str) -> Self {
        Self::new(vec![(StatusCode::OK, bytes::Bytes::from(body.to_string()))])
    }

    /// A sequence of `200 OK` JSON bodies, one per upstream page.
    pub fn ok_pages(bodies: &[&str]) -> Self {
        Self::new(
            bodies
                .iter()
                .map(|b| (StatusCode::OK, bytes::Bytes::from(b.to_string())))
                .collect(),
        )
    }

    /// A single non-success status with an empty body.
    pub fn status(code: u16) -> Self {
        Self::new(vec![(
            StatusCode::from_u16(code).unwrap(),
            bytes::Bytes::new(),
        )])
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    pub fn last_call(&self) -> RecordedCall {
        self.calls().last().expect("no calls recorded").clone()
    }
}

#[async_trait]
impl HttpFetch for StubFetch {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        query: &[(&str, &str)],
    ) -> Result<(StatusCode, bytes::Bytes)> {
        self.calls.lock().unwrap().push(RecordedCall {
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            query: query
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
        let mut next = self.next.lock().unwrap();
        let i = (*next).min(self.responses.len().saturating_sub(1));
        *next += 1;
        Ok(self.responses[i].clone())
    }
}
