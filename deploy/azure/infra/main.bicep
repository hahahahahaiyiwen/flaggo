targetScope = 'subscription'

@minLength(1)
@maxLength(64)
param environmentName string

@minLength(1)
param location string

param sessionId string
param deployedBy string
param createdAt string
param deployerObjectId string

@minLength(1)
param vmAdminUsername string

@secure()
@minLength(32)
param vmSshPublicKey string

@minLength(1)
param vmImagePublisher string

@minLength(1)
param vmImageOffer string

@minLength(1)
param vmImageSku string

@minLength(1)
param vmImageVersion string

param enableVmKeyVaultAccess bool = false
param enableVmBlobAccess bool = false

var tags = {
  'app-onboard-skill': 'true'
  'app-onboard-session-id': sessionId
  'created-at': createdAt
  environment: environmentName
  'deployed-by': deployedBy
}

resource rg 'Microsoft.Resources/resourceGroups@2026-06-01' = {
  name: 'rg-flaggosvc-dev-bf57'
  location: location
  tags: tags
}

module network './modules/network.bicep' = {
  name: 'network'
  scope: rg
  params: {
    location: location
    tags: tags
    virtualNetworkName: 'vnet-flaggosvc-dev-bf57'
    networkSecurityGroupName: 'nsg-flaggosvc-dev-bf57'
    publicIpName: 'pip-flaggosvc-dev-bf57'
    networkInterfaceName: 'nic-flaggosvc-dev-bf57'
  }
}

module dataDisk './modules/data-disk.bicep' = {
  name: 'data-disk'
  scope: rg
  params: {
    location: location
    tags: tags
    dataDiskName: 'data-flaggosvc-dev-bf57'
  }
}

module storage './modules/storage.bicep' = {
  name: 'storage'
  scope: rg
  params: {
    location: location
    tags: tags
    storageAccountName: 'stflaggosvcdevbf57'
  }
}

module keyVault './modules/key-vault.bicep' = {
  name: 'key-vault'
  scope: rg
  params: {
    location: location
    tags: tags
    keyVaultName: 'kv-flaggosvc-dev-bf57'
  }
}

module virtualMachine './modules/virtual-machine.bicep' = {
  name: 'virtual-machine'
  scope: rg
  params: {
    location: location
    tags: tags
    virtualMachineName: 'vm-flaggosvc-dev-bf57'
    osDiskName: 'osdisk-flaggosvc-dev-bf57'
    networkInterfaceId: network.outputs.networkInterfaceId
    dataDiskId: dataDisk.outputs.dataDiskId
    vmAdminUsername: vmAdminUsername
    vmSshPublicKey: vmSshPublicKey
    vmImagePublisher: vmImagePublisher
    vmImageOffer: vmImageOffer
    vmImageSku: vmImageSku
    vmImageVersion: vmImageVersion
  }
}

module roleAssignments './modules/role-assignments.bicep' = {
  name: 'role-assignments'
  scope: rg
  params: {
    storageAccountName: 'stflaggosvcdevbf57'
    enableVmBlobAccess: enableVmBlobAccess
    keyVaultId: keyVault.outputs.keyVaultId
    keyVaultName: 'kv-flaggosvc-dev-bf57'
    deployerObjectId: deployerObjectId
    enableVmKeyVaultAccess: enableVmKeyVaultAccess
    vmPrincipalId: virtualMachine.outputs.principalId
  }
  dependsOn: [
    storage
  ]
}
