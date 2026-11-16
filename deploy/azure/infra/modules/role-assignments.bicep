param keyVaultId string
param keyVaultName string
param deployerObjectId string
param enableVmKeyVaultAccess bool
param vmPrincipalId string
param storageAccountName string
param enableVmBlobAccess bool

var keyVaultSecretsOfficerRoleId = 'b86a8fe4-44ce-4948-aee5-eccb2c155cd7'
var keyVaultSecretsUserRoleId = '4633458b-17de-408a-b874-0445c86b69e6'
var blobReaderRoleId = '2a2b9908-6ea1-4ae2-8e65-a410df84e7d1'
var blobContributorRoleId = 'ba92f5b4-2d11-453d-a403-e96b0029c9fe'

resource keyVault 'Microsoft.KeyVault/vaults@2026-05-15' existing = {
  name: keyVaultName
}

resource deployerSecretsOfficer 'Microsoft.Authorization/roleAssignments@2022-04-01' = if (!empty(deployerObjectId)) {
  name: guid(keyVaultId, deployerObjectId, keyVaultSecretsOfficerRoleId)
  scope: keyVault
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', keyVaultSecretsOfficerRoleId)
    principalId: deployerObjectId
    principalType: 'User'
  }
}

resource vmSecretsUser 'Microsoft.Authorization/roleAssignments@2022-04-01' = if (enableVmKeyVaultAccess) {
  name: guid(keyVaultId, vmPrincipalId, keyVaultSecretsUserRoleId)
  scope: keyVault
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', keyVaultSecretsUserRoleId)
    principalId: vmPrincipalId
    principalType: 'ServicePrincipal'
  }
}

resource artifactContainer 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' existing = {
  name: '${storageAccountName}/default/release-artifacts'
}

resource checksumContainer 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' existing = {
  name: '${storageAccountName}/default/checksums'
}

resource backupContainer 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' existing = {
  name: '${storageAccountName}/default/backups'
}

resource vmArtifactReader 'Microsoft.Authorization/roleAssignments@2022-04-01' = if (enableVmBlobAccess) {
  name: guid(artifactContainer.id, vmPrincipalId, blobReaderRoleId)
  scope: artifactContainer
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', blobReaderRoleId)
    principalId: vmPrincipalId
    principalType: 'ServicePrincipal'
  }
}

resource vmChecksumReader 'Microsoft.Authorization/roleAssignments@2022-04-01' = if (enableVmBlobAccess) {
  name: guid(checksumContainer.id, vmPrincipalId, blobReaderRoleId)
  scope: checksumContainer
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', blobReaderRoleId)
    principalId: vmPrincipalId
    principalType: 'ServicePrincipal'
  }
}

resource vmBackupContributor 'Microsoft.Authorization/roleAssignments@2022-04-01' = if (enableVmBlobAccess) {
  name: guid(backupContainer.id, vmPrincipalId, blobContributorRoleId)
  scope: backupContainer
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', blobContributorRoleId)
    principalId: vmPrincipalId
    principalType: 'ServicePrincipal'
  }
}
