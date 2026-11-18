use serde::{Deserialize, Serialize};

use crate::{AuthorityScope, ContractError, DecisionContract};

/// Explicit resource namespace, independent of authentication credentials.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceScope {
    pub tenant: String,
    pub application: String,
    pub environment: String,
}

impl ResourceScope {
    pub fn new(
        tenant: String,
        application: String,
        environment: String,
    ) -> Result<Self, ContractError> {
        let scope = Self {
            tenant,
            application,
            environment,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        for (field, value) in [
            ("tenant", &self.tenant),
            ("application", &self.application),
            ("environment", &self.environment),
        ] {
            validate_resource_identifier(field, value)?;
        }
        Ok(())
    }
}

impl TryFrom<&AuthorityScope> for ResourceScope {
    type Error = ContractError;

    fn try_from(value: &AuthorityScope) -> Result<Self, Self::Error> {
        Self::new(
            value
                .tenant
                .clone()
                .ok_or_else(|| ContractError::InvalidModel("tenant is required".into()))?,
            value
                .application
                .clone()
                .ok_or_else(|| ContractError::InvalidModel("application is required".into()))?,
            value
                .environment
                .clone()
                .ok_or_else(|| ContractError::InvalidModel("environment is required".into()))?,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractResourceId {
    pub scope: ResourceScope,
    pub name: String,
}

impl ContractResourceId {
    pub fn from_resource_key(key: &str) -> Result<Self, ContractError> {
        let parts: Vec<_> = key.split('/').collect();
        let [tenant, application, environment, name] = parts.as_slice() else {
            return Err(ContractError::InvalidModel(
                "Invalid scoped resource key.".into(),
            ));
        };
        Self::new(
            ResourceScope::new(
                (*tenant).into(),
                (*application).into(),
                (*environment).into(),
            )?,
            (*name).into(),
        )
    }

    pub fn new(scope: ResourceScope, name: String) -> Result<Self, ContractError> {
        scope.validate()?;
        validate_resource_identifier("contractName", &name)?;
        Ok(Self { scope, name })
    }

    /// Validated identifiers cannot contain '/', making this key unambiguous.
    pub fn resource_key(&self) -> Result<String, ContractError> {
        self.scope.validate()?;
        validate_resource_identifier("contractName", &self.name)?;
        Ok(format!(
            "{}/{}/{}/{}",
            self.scope.tenant, self.scope.application, self.scope.environment, self.name
        ))
    }
}

impl TryFrom<&DecisionContract> for ContractResourceId {
    type Error = ContractError;

    fn try_from(contract: &DecisionContract) -> Result<Self, Self::Error> {
        Self::new(
            ResourceScope::try_from(&contract.authority)?,
            contract.name.clone(),
        )
    }
}

fn validate_resource_identifier(field: &str, value: &str) -> Result<(), ContractError> {
    if !crate::validation::is_decision_name(value) {
        return Err(ContractError::InvalidModel(format!(
            "{field} must match ^[A-Za-z][A-Za-z0-9._-]{{0,127}}$"
        )));
    }
    Ok(())
}
