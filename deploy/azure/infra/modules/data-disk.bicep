param location string
param tags object
param dataDiskName string

resource dataDisk 'Microsoft.Compute/disks@2026-10-02' = {
  name: dataDiskName
  location: location
  tags: tags
  sku: {
    name: 'StandardSSD_LRS'
  }
  properties: {
    creationData: {
      createOption: 'Empty'
    }
    diskSizeGB: 64
    osType: 'Linux'
  }
}

output dataDiskId string = dataDisk.id
