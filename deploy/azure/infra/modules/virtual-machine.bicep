param location string
param tags object
param virtualMachineName string
param osDiskName string
param networkInterfaceId string
param dataDiskId string
param vmAdminUsername string
@secure()
param vmSshPublicKey string
param vmImagePublisher string
param vmImageOffer string
param vmImageSku string
param vmImageVersion string

resource virtualMachine 'Microsoft.Compute/virtualMachines@2026-04-01' = {
  name: virtualMachineName
  location: location
  tags: tags
  identity: {
    type: 'SystemAssigned'
  }
  properties: {
    hardwareProfile: {
      vmSize: 'Standard_D2as_v5'
    }
    storageProfile: {
      imageReference: {
        publisher: vmImagePublisher
        offer: vmImageOffer
        sku: vmImageSku
        version: vmImageVersion
      }
      osDisk: {
        name: osDiskName
        createOption: 'FromImage'
        diskSizeGB: 32
        managedDisk: {
          storageAccountType: 'StandardSSD_LRS'
        }
        deleteOption: 'Delete'
        osType: 'Linux'
      }
      dataDisks: [
        {
          lun: 0
          name: 'data-flaggosvc-dev-bf57'
          createOption: 'Attach'
          managedDisk: {
            id: dataDiskId
          }
          caching: 'None'
          deleteOption: 'Detach'
        }
      ]
    }
    osProfile: {
      computerName: virtualMachineName
      adminUsername: vmAdminUsername
      linuxConfiguration: {
        disablePasswordAuthentication: true
        provisionVMAgent: true
        patchSettings: {
          assessmentMode: 'ImageDefault'
          patchMode: 'ImageDefault'
        }
        ssh: {
          publicKeys: [
            {
              path: '/home/${vmAdminUsername}/.ssh/authorized_keys'
              keyData: vmSshPublicKey
            }
          ]
        }
      }
    }
    networkProfile: {
      networkInterfaces: [
        {
          id: networkInterfaceId
          properties: {
            primary: true
          }
        }
      ]
    }
  }
}

output principalId string = virtualMachine.identity.principalId
