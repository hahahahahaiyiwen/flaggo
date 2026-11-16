param location string
param tags object
param storageAccountName string

resource storageAccount 'Microsoft.Storage/storageAccounts@2026-09-01' = {
  name: storageAccountName
  location: location
  tags: tags
  kind: 'StorageV2'
  sku: {
    name: 'Standard_LRS'
  }
  properties: {
    accessTier: 'Hot'
    allowBlobPublicAccess: false
    minimumTlsVersion: 'TLS1_2'
    publicNetworkAccess: 'Enabled'
    supportsHttpsTrafficOnly: true
  }
}

resource blobService 'Microsoft.Storage/storageAccounts/blobServices@2026-09-01' = {
  parent: storageAccount
  name: 'default'
}

resource releaseArtifacts 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' = {
  parent: blobService
  name: 'release-artifacts'
  properties: {
    publicAccess: 'None'
  }
}

resource checksums 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' = {
  parent: blobService
  name: 'checksums'
  properties: {
    publicAccess: 'None'
  }
}

resource backups 'Microsoft.Storage/storageAccounts/blobServices/containers@2026-09-01' = {
  parent: blobService
  name: 'backups'
  properties: {
    publicAccess: 'None'
  }
}
