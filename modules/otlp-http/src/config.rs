pub const DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OtlpReceiverConfig {
    maximum_decompressed_request_bytes: usize,
    pub(crate) expose_inbox_health: bool,
}

impl OtlpReceiverConfig {
    pub fn new(maximum_decompressed_request_bytes: usize) -> Result<Self, String> {
        if maximum_decompressed_request_bytes == 0
            || maximum_decompressed_request_bytes == usize::MAX
        {
            return Err(
                "maximum decompressed OTLP request bytes must be between 1 and usize::MAX - 1"
                    .into(),
            );
        }
        Ok(Self {
            maximum_decompressed_request_bytes,
            expose_inbox_health: false,
        })
    }

    /// Opt in only at a trusted-local listener; public health must not reveal inventory.
    #[must_use]
    pub fn with_local_health_details(mut self) -> Self {
        self.expose_inbox_health = true;
        self
    }

    #[must_use]
    pub const fn maximum_decompressed_request_bytes(self) -> usize {
        self.maximum_decompressed_request_bytes
    }
}

impl Default for OtlpReceiverConfig {
    fn default() -> Self {
        Self {
            maximum_decompressed_request_bytes: DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES,
            expose_inbox_health: false,
        }
    }
}
