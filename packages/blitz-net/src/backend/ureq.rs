use blitz_traits::net::{Request, Response};

use super::{BackendError, RequestBackend};

impl From<ureq::Error> for BackendError {
    fn from(e: ureq::Error) -> Self {
        BackendError {
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    client: ureq::Agent,
}

impl RequestBackend for Backend {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            client: ureq::agent(),
        }
    }

    async fn request(&mut self, request: Request) -> Result<Response, BackendError> {
        let mut response = self.client.run(request.into())?;

        let status = response.status().as_u16();

        Ok(Response {
            status,
            headers: response.headers().clone(),
            body: response.body_mut().read_to_vec()?.into(),
        })
    }
}
