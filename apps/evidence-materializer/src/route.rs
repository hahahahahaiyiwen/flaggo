use flaggo_evidence_store::{AuthorityScope, SourceKey};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MaterializationRoute {
    pub authority: AuthorityScope,
    pub source: SourceKey,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CurrentContractKey {
    pub authority: AuthorityScope,
    pub contract_name: String,
}
