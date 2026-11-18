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
pub fn workspace_id(resource: &flaggo_contract::ContractResourceId) -> String {
    sha256_digest(
        format!(
            "flaggo.workspace/v2\n{}/{}/{}/{}",
            resource.scope.tenant,
            resource.scope.application,
            resource.scope.environment,
            resource.name,
        )
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::workspace_id;

    #[test]
    fn workspace_identity_separates_full_resource_scopes() {
        let scope =
            flaggo_contract::ResourceScope::new("local".into(), "checkout".into(), "test".into())
                .unwrap();
        let first =
            flaggo_contract::ContractResourceId::new(scope, "checkout.delay".into()).unwrap();
        let mut second = first.clone();
        second.scope.environment = "production".into();
        assert_ne!(workspace_id(&first), workspace_id(&second));
    }
}
