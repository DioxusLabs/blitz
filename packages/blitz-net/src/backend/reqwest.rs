use super::{BackendError, RequestBackend, Response};
use blitz_traits::net::Request;

// Compat with reqwest
impl From<reqwest::Error> for BackendError {
    fn from(e: reqwest::Error) -> Self {
        BackendError {
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    client: reqwest::Client,
}

impl RequestBackend for Backend {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            client: reqwest::Client::new(),
        }
    }

    async fn request(&mut self, request: Request) -> Result<Response, BackendError> {
        let request = self
            .client
            .request(request.method, request.url.clone())
            .headers(request.headers);

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await?;

        Ok(Response {
            status: status.as_u16(),
            headers,
            body,
        })
    }
}
