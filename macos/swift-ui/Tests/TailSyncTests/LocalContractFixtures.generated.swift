// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.
import Foundation
@testable import TailSync

func validateGeneratedFixture(_ contract: String, data: Data) throws {
  switch contract {
  case "ActiveRoute": _ = try JSONDecoder().decode(ContractActiveRoute.self, from: data)
  case "ConnectionInterface": _ = try JSONDecoder().decode(ContractConnectionInterface.self, from: data)
  case "DaemonStatus": _ = try JSONDecoder().decode(ContractDaemonStatus.self, from: data)
  case "FavoriteMutation": _ = try JSONDecoder().decode(ContractFavoriteMutation.self, from: data)
  case "FileProgress": _ = try JSONDecoder().decode(ContractFileProgress.self, from: data)
  case "HistoryEntry": _ = try JSONDecoder().decode(ContractHistoryEntry.self, from: data)
  case "HistoryQueryPage": _ = try JSONDecoder().decode(ContractHistoryQueryPage.self, from: data)
  case "LocalCapabilities": _ = try JSONDecoder().decode(ContractLocalCapabilities.self, from: data)
  case "LocalDeviceSnapshot": _ = try JSONDecoder().decode(ContractLocalDeviceSnapshot.self, from: data)
  case "MacRuntimeSnapshot": _ = try JSONDecoder().decode(ContractMacRuntimeSnapshot.self, from: data)
  case "PeerCandidate": _ = try JSONDecoder().decode(ContractPeerCandidate.self, from: data)
  case "PeerRouteSnapshot": _ = try JSONDecoder().decode(ContractPeerRouteSnapshot.self, from: data)
  case "PeerSnapshot": _ = try JSONDecoder().decode(ContractPeerSnapshot.self, from: data)
  case "PeerStatus": _ = try JSONDecoder().decode(ContractPeerStatus.self, from: data)
  case "PeersResponse": _ = try JSONDecoder().decode(ContractPeersResponse.self, from: data)
  case "PreviewBatchNavigation": _ = try JSONDecoder().decode(ContractPreviewBatchNavigation.self, from: data)
  case "PreviewErrorCode": _ = try JSONDecoder().decode(ContractPreviewErrorCode.self, from: data)
  case "PreviewErrorInfo": _ = try JSONDecoder().decode(ContractPreviewErrorInfo.self, from: data)
  case "PreviewFrameMetadata": _ = try JSONDecoder().decode(ContractPreviewFrameMetadata.self, from: data)
  case "PreviewKind": _ = try JSONDecoder().decode(ContractPreviewKind.self, from: data)
  case "RuntimeNotification": _ = try JSONDecoder().decode(ContractRuntimeNotification.self, from: data)
  case "StorageStatus": _ = try JSONDecoder().decode(ContractStorageStatus.self, from: data)
  case "SyncWarning": _ = try JSONDecoder().decode(ContractSyncWarning.self, from: data)
  case "WindowsRuntimeSnapshot": _ = try JSONDecoder().decode(ContractWindowsRuntimeSnapshot.self, from: data)
  default: throw NSError(domain: "Unknown contract fixture", code: 1)
  }
}
