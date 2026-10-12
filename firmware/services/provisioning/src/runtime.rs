use crate::{CredentialStore, Credentials, ProvisioningConfig};

/// Hardware/network operations required by the transport-independent lifecycle.
///
/// Both the browser portal and optional HIL adapter ultimately return from
/// [`wait_for_credentials`](Self::wait_for_credentials), so connection and
/// persistence behavior cannot diverge between the two transports.
#[allow(async_fn_in_trait)]
pub trait ProvisioningBackend {
    type Error;

    async fn connect(
        &mut self,
        credentials: &Credentials,
        timeout_seconds: u16,
    ) -> Result<(), Self::Error>;

    async fn delay_seconds(&mut self, seconds: u16);

    async fn wait_for_credentials(&mut self) -> Result<Credentials, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProvisionError<BackendError, StoreError> {
    Backend(BackendError),
    Store(StoreError),
}

/// Connect using saved credentials or run the portal until valid credentials
/// connect. Newly submitted credentials are saved only after connection.
pub async fn provision<B, S>(
    backend: &mut B,
    store: &mut S,
    config: ProvisioningConfig,
    force_portal: bool,
) -> Result<Credentials, ProvisionError<B::Error, S::Error>>
where
    B: ProvisioningBackend,
    S: CredentialStore,
{
    if force_portal {
        store.clear().map_err(ProvisionError::Store)?;
    } else if let Some(saved) = store.load().map_err(ProvisionError::Store)? {
        for attempt in 0..config.saved_attempts {
            if backend
                .connect(&saved, config.connect_timeout_seconds)
                .await
                .is_ok()
            {
                return Ok(saved);
            }
            if attempt + 1 < config.saved_attempts {
                backend.delay_seconds(1_u16 << attempt.min(3)).await;
            }
        }
    }

    loop {
        let submitted = backend
            .wait_for_credentials()
            .await
            .map_err(ProvisionError::Backend)?;
        if backend
            .connect(&submitted, config.connect_timeout_seconds)
            .await
            .is_ok()
        {
            store.save(&submitted).map_err(ProvisionError::Store)?;
            return Ok(submitted);
        }
    }
}

#[cfg(test)]
mod tests {
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use heapless::Vec;

    use super::*;

    #[derive(Default)]
    struct MemoryStore(Option<Credentials>);

    impl CredentialStore for MemoryStore {
        type Error = ();

        fn load(&mut self) -> Result<Option<Credentials>, Self::Error> {
            Ok(self.0.clone())
        }

        fn save(&mut self, credentials: &Credentials) -> Result<(), Self::Error> {
            self.0 = Some(credentials.clone());
            Ok(())
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.0 = None;
            Ok(())
        }
    }

    struct Backend {
        attempts: Vec<bool, 8>,
        submitted: Credentials,
        portal_visits: usize,
    }

    impl ProvisioningBackend for Backend {
        type Error = ();

        async fn connect(
            &mut self,
            _credentials: &Credentials,
            _timeout_seconds: u16,
        ) -> Result<(), Self::Error> {
            if self.attempts.remove(0) {
                Ok(())
            } else {
                Err(())
            }
        }

        async fn delay_seconds(&mut self, _seconds: u16) {}

        async fn wait_for_credentials(&mut self) -> Result<Credentials, Self::Error> {
            self.portal_visits += 1;
            Ok(self.submitted.clone())
        }
    }

    #[test]
    fn falls_back_then_saves_only_successful_submission() {
        let saved = Credentials::new("old", "old-password").unwrap();
        let submitted = Credentials::new("new", "new-password").unwrap();
        let mut store = MemoryStore(Some(saved));
        let mut backend = Backend {
            attempts: Vec::from_slice(&[false, false, false, true]).unwrap(),
            submitted: submitted.clone(),
            portal_visits: 0,
        };
        let result = block_on(provision(
            &mut backend,
            &mut store,
            ProvisioningConfig::default(),
            false,
        ))
        .unwrap();
        assert_eq!(result, submitted);
        assert_eq!(store.0, Some(submitted));
        assert_eq!(backend.portal_visits, 1);
    }

    fn block_on(
        future: impl Future<Output = Result<Credentials, ProvisionError<(), ()>>>,
    ) -> Result<Credentials, ProvisionError<(), ()>> {
        let mut future = pin!(future);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
        }
    }
}
