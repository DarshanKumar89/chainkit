'use strict'
// Load the platform-specific native module
const { existsSync, readFileSync } = require('fs')
const { join } = require('path')

const { platform, arch } = process

let nativeBinding = null
let localFileExisted = false
let loadError = null

function isMusl() {
  if (!process.report || typeof process.report.getReport !== 'function') {
    try {
      const lddPath = require('child_process').execSync('which ldd').toString().trim()
      return readFileSync(lddPath, 'utf8').includes('musl')
    } catch (e) {
      return true
    }
  } else {
    const { glibcVersionRuntime } = process.report.getReport().header
    return !glibcVersionRuntime
  }
}

switch (platform) {
  case 'android':
    switch (arch) {
      case 'arm64':
        localFileExisted = existsSync(join(__dirname, 'chaincodec.android-arm64.node'))
        try {
          if (localFileExisted) {
            nativeBinding = require('./chaincodec.android-arm64.node')
          } else {
            nativeBinding = require('@chainfoundry/chaincodec-android-arm64')
          }
        } catch (e) {
          loadError = e
        }
        break
      default:
        throw new Error(`Unsupported architecture on Android ${arch}`)
    }
    break
  case 'win32':
    switch (arch) {
      case 'x64':
        localFileExisted = existsSync(join(__dirname, 'chaincodec.win32-x64-msvc.node'))
        try {
          if (localFileExisted) {
            nativeBinding = require('./chaincodec.win32-x64-msvc.node')
          } else {
            nativeBinding = require('@chainfoundry/chaincodec-win32-x64-msvc')
          }
        } catch (e) {
          loadError = e
        }
        break
      case 'arm64':
        localFileExisted = existsSync(join(__dirname, 'chaincodec.win32-arm64-msvc.node'))
        try {
          if (localFileExisted) {
            nativeBinding = require('./chaincodec.win32-arm64-msvc.node')
          } else {
            nativeBinding = require('@chainfoundry/chaincodec-win32-arm64-msvc')
          }
        } catch (e) {
          loadError = e
        }
        break
      default:
        throw new Error(`Unsupported architecture on Windows: ${arch}`)
    }
    break
  case 'darwin':
    localFileExisted = existsSync(join(__dirname, 'chaincodec.darwin-universal.node'))
    try {
      if (localFileExisted) {
        nativeBinding = require('./chaincodec.darwin-universal.node')
      } else {
        nativeBinding = require('@chainfoundry/chaincodec-darwin-universal')
      }
    } catch {}
    if (!nativeBinding) {
      switch (arch) {
        case 'x64':
          localFileExisted = existsSync(join(__dirname, 'chaincodec.darwin-x64.node'))
          try {
            if (localFileExisted) {
              nativeBinding = require('./chaincodec.darwin-x64.node')
            } else {
              nativeBinding = require('@chainfoundry/chaincodec-darwin-x64')
            }
          } catch (e) {
            loadError = e
          }
          break
        case 'arm64':
          localFileExisted = existsSync(join(__dirname, 'chaincodec.darwin-arm64.node'))
          try {
            if (localFileExisted) {
              nativeBinding = require('./chaincodec.darwin-arm64.node')
            } else {
              nativeBinding = require('@chainfoundry/chaincodec-darwin-arm64')
            }
          } catch (e) {
            loadError = e
          }
          break
        default:
          throw new Error(`Unsupported architecture on macOS: ${arch}`)
      }
    }
    break
  case 'linux':
    switch (arch) {
      case 'x64':
        if (isMusl()) {
          localFileExisted = existsSync(join(__dirname, 'chaincodec.linux-x64-musl.node'))
          try {
            if (localFileExisted) {
              nativeBinding = require('./chaincodec.linux-x64-musl.node')
            } else {
              nativeBinding = require('@chainfoundry/chaincodec-linux-x64-musl')
            }
          } catch (e) {
            loadError = e
          }
        } else {
          localFileExisted = existsSync(join(__dirname, 'chaincodec.linux-x64-gnu.node'))
          try {
            if (localFileExisted) {
              nativeBinding = require('./chaincodec.linux-x64-gnu.node')
            } else {
              nativeBinding = require('@chainfoundry/chaincodec-linux-x64-gnu')
            }
          } catch (e) {
            loadError = e
          }
        }
        break
      case 'arm64':
        localFileExisted = existsSync(join(__dirname, 'chaincodec.linux-arm64-gnu.node'))
        try {
          if (localFileExisted) {
            nativeBinding = require('./chaincodec.linux-arm64-gnu.node')
          } else {
            nativeBinding = require('@chainfoundry/chaincodec-linux-arm64-gnu')
          }
        } catch (e) {
          loadError = e
        }
        break
      default:
        throw new Error(`Unsupported architecture on Linux: ${arch}`)
    }
    break
  default:
    throw new Error(`Unsupported OS: ${platform}, architecture: ${arch}`)
}

if (!nativeBinding) {
  if (loadError) {
    throw loadError
  }
  throw new Error('Failed to load native @chainfoundry/chaincodec binding')
}

const { MemoryRegistry, EvmDecoder, EvmCallDecoder, EvmEncoder, Eip712Parser } = nativeBinding

module.exports.MemoryRegistry = MemoryRegistry
module.exports.EvmDecoder = EvmDecoder
module.exports.EvmCallDecoder = EvmCallDecoder
module.exports.EvmEncoder = EvmEncoder
module.exports.Eip712Parser = Eip712Parser
