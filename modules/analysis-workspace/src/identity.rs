use sha2::{Digest, Sha256};

#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[must_use]
pub fn workspace_id(contract_name: &str) -> String {
    sha256_digest(contract_name.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::workspace_id;

    #[test]
    fn workspace_identity_is_stable_and_name_only() {
        assert_eq!(
            workspace_id("checkout.delay"),
            "sha256:8f74b99894daf4f5601abb968848f63a55c1800789daaaa228a859c6d162877b"
        );
    }
}
