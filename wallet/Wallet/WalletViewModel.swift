import Foundation
import os
import Erc7730
import ReownWalletKit

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "WalletViewModel")

@Observable
final class WalletViewModel {

    // Key management
    var privateKeyHex = ""
    var ethereumAddress: String?
    var keyError: String?

    // WalletConnect
    var pairingURI = ""
    var isPaired = false
    var pairingError: String?

    // Sessions
    var activeSessions: [Session] = []

    // Proposal
    var pendingProposal: Session.Proposal?
    var showProposal = false

    // Request
    var pendingRequest: Request?
    var displayModel: DisplayModel?
    var requestError: String?
    var rawRequestJSON: String?
    var showRequest = false

    // QR
    var showScanner = false

    private var keyManager: KeyManager?
    private let clearSigning = ClearSigningService()
    private let wc = WalletConnectService.shared
    var wcConfigured = false

    init() {
        if let restored = KeyManager.restore() {
            keyManager = restored
            ethereumAddress = restored.ethereumAddress
        }
    }

    // MARK: - Key Import

    func importKey() {
        keyError = nil
        do {
            let km = try KeyManager(privateKeyHex: privateKeyHex)
            try km.save()
            keyManager = km
            ethereumAddress = km.ethereumAddress
            privateKeyHex = ""
        } catch {
            keyError = error.localizedDescription
        }
    }

    func clearKey() {
        KeyManager.clear()
        keyManager = nil
        ethereumAddress = nil
        privateKeyHex = ""
        Task {
            await wc.disconnectAllSessions()
            await MainActor.run {
                activeSessions = []
                pendingProposal = nil
                pendingRequest = nil
                displayModel = nil
                requestError = nil
                rawRequestJSON = nil
                showProposal = false
                showRequest = false
            }
        }
    }

    // MARK: - WalletConnect

    func configureWalletConnect() {
        let projectId = Bundle.main.infoDictionary?["WalletConnectProjectID"] as? String ?? ""
        guard !projectId.isEmpty, projectId != "YOUR_PROJECT_ID_HERE" else {
            log.warning("WalletConnect project ID not set — skipping configuration")
            return
        }
        log.info("Configuring WalletConnect with project ID: \(projectId.prefix(8))...")
        Task {
            await wc.configure(projectId: projectId)
            log.info("WalletConnect configured successfully")
            await MainActor.run { wcConfigured = true }
            listenForProposals()
            listenForRequests()
            listenForSessionDeletes()
            refreshSessions()
        }
    }

    func pair() {
        pairingError = nil
        let uri = pairingURI.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !uri.isEmpty else { return }
        log.info("Pairing with URI: \(uri.prefix(30))...")
        Task {
            do {
                try await wc.pair(uri: uri)
                log.info("Pairing succeeded")
                await MainActor.run {
                    isPaired = true
                    pairingURI = ""
                }
            } catch {
                log.error("Pairing failed: \(error)")
                await MainActor.run { pairingError = error.localizedDescription }
            }
        }
    }

    func pairFromQR(_ code: String) {
        pairingURI = code
        showScanner = false
        pair()
    }

    // MARK: - Proposal

    func approveProposal() {
        guard let proposal = pendingProposal, let address = ethereumAddress else { return }
        log.info("Approving proposal from \(proposal.proposer.name) with address \(address)")
        Task {
            do {
                try await wc.approveProposal(proposal, address: address)
                log.info("Proposal approved")
                await MainActor.run {
                    showProposal = false
                    pendingProposal = nil
                    refreshSessions()
                }
            } catch {
                log.error("Approve proposal failed: \(error)")
                await MainActor.run { pairingError = error.localizedDescription }
            }
        }
    }

    func rejectProposal() {
        guard let proposal = pendingProposal else { return }
        log.info("Rejecting proposal from \(proposal.proposer.name)")
        Task {
            try? await wc.rejectProposal(proposal)
            await MainActor.run {
                showProposal = false
                pendingProposal = nil
            }
        }
    }

    // MARK: - Request

    func processRequest(_ request: Request) {
        log.info("Received request: method=\(request.method) topic=\(request.topic.prefix(8))...")
        pendingRequest = request
        displayModel = nil
        requestError = nil
        rawRequestJSON = nil

        let method = request.method

        if method == "eth_sendTransaction" {
            processTransaction(request)
        } else if method == "eth_signTypedData" || method == "eth_signTypedData_v4" {
            processTypedData(request)
        } else {
            log.warning("Unsupported method: \(method)")
            rawRequestJSON = prettyJSON(request.params)
            requestError = "Unsupported method: \(method)"
        }
        showRequest = true
    }

    func rejectRequest() {
        guard let request = pendingRequest else { return }
        Task {
            try? await wc.rejectRequest(request)
            await MainActor.run {
                showRequest = false
                pendingRequest = nil
                displayModel = nil
                requestError = nil
                rawRequestJSON = nil
            }
        }
    }

    func approveRequest() {
        guard let request = pendingRequest else { return }
        guard let keyManager else {
            requestError = "No private key available"
            return
        }

        Task {
            do {
                let signer = EvmSigningService.shared
                let expectedAddress = keyManager.ethereumAddress
                let responseValue: AnyCodable

                switch request.method {
                case "eth_sendTransaction":
                    let txHash = try await signer.signAndSend(
                        request: request,
                        privateKeyHex: keyManager.privateKeyHex,
                        expectedAddress: expectedAddress
                    )
                    responseValue = AnyCodable(txHash)

                case "eth_signTypedData", "eth_signTypedData_v4":
                    let signature = try signer.signTypedData(
                        request: request,
                        privateKeyHex: keyManager.privateKeyHex,
                        expectedAddress: expectedAddress
                    )
                    responseValue = AnyCodable(signature)

                default:
                    throw EvmSigningService.SigningError.invalidParams("unsupported method \(request.method)")
                }

                try await WalletKit.instance.respond(
                    topic: request.topic,
                    requestId: request.id,
                    response: .response(responseValue)
                )

                await MainActor.run {
                    showRequest = false
                    pendingRequest = nil
                    displayModel = nil
                    requestError = nil
                    rawRequestJSON = nil
                }
            } catch {
                let rpcError: JSONRPCError
                switch error {
                case EvmSigningService.SigningError.addressMismatch(_, _):
                    rpcError = JSONRPCError(code: 4001, message: "User rejected")
                case EvmSigningService.SigningError.invalidParams(_):
                    rpcError = JSONRPCError.invalidParams
                default:
                    rpcError = JSONRPCError(code: -32000, message: error.localizedDescription)
                }

                try? await WalletKit.instance.respond(
                    topic: request.topic,
                    requestId: request.id,
                    response: .error(rpcError)
                )

                await MainActor.run {
                    requestError = error.localizedDescription
                }
            }
        }
    }

    func refreshSessions() {
        guard wcConfigured else { return }
        Task {
            let sessions = await wc.sessions
            await MainActor.run { activeSessions = sessions }
        }
    }

    func disconnectSession(_ session: Session) {
        Task {
            do {
                try await wc.disconnect(topic: session.topic)
                await MainActor.run {
                    activeSessions.removeAll { $0.topic == session.topic }
                }
            } catch {
                log.error("Disconnect failed: \(error)")
            }
        }
    }

    // MARK: - Private

    private func processTransaction(_ request: Request) {
        guard let paramsArray = try? request.params.get([TransactionParams].self),
              let tx = paramsArray.first else {
            log.error("Could not parse transaction params")
            requestError = "Could not parse transaction params"
            rawRequestJSON = prettyJSON(request.params)
            return
        }

        rawRequestJSON = prettyJSON(request.params)

        guard let to = tx.to else {
            requestError = "Transaction has no recipient (contract creation)"
            return
        }

        let chainRef = request.chainId
        let chainId = UInt64(chainRef.reference) ?? 1

        let calldata = tx.data ?? tx.input ?? "0x"
        log.info("Processing tx: to=\(to) chainId=\(chainId) calldata=\(calldata.prefix(10))...")

        Task {
            let result = await clearSigning.formatCalldata(
                chainId: chainId,
                to: to,
                calldata: calldata,
                value: tx.value,
                from: tx.from
            )
            await MainActor.run {
                switch result {
                case .success(let model):
                    log.info("Clear signing OK: intent=\(model.intent) entries=\(model.entries.count)")
                    displayModel = model
                case .failure(let error):
                    log.error("Clear signing failed: \(error)")
                    requestError = error.localizedDescription
                }
            }
        }
    }

    private func processTypedData(_ request: Request) {
        do {
            let payload = try EvmSigningService.shared.extractTypedDataPayload(from: request.params)
            let typedDataJson = payload.json
            rawRequestJSON = typedDataJson

            Task {
                let result = await clearSigning.formatTypedData(typedDataJson: typedDataJson)
                await MainActor.run {
                    switch result {
                    case .success(let model):
                        displayModel = model
                    case .failure(let error):
                        requestError = error.localizedDescription
                    }
                }
            }
        } catch {
            requestError = error.localizedDescription
            rawRequestJSON = prettyJSON(request.params)
        }
    }

    private func listenForProposals() {
        Task {
            log.info("Listening for session proposals")
            for await proposal in wc.sessionProposals {
                log.info("Received proposal from \(proposal.proposer.name)")
                await MainActor.run {
                    pendingProposal = proposal
                    showProposal = true
                }
            }
        }
    }

    private func listenForRequests() {
        Task {
            log.info("Listening for session requests")
            for await request in wc.sessionRequests {
                log.info("Received request: \(request.method)")
                await MainActor.run {
                    processRequest(request)
                }
            }
        }
    }

    private func listenForSessionDeletes() {
        Task {
            log.info("Listening for session deletes")
            for await delete in wc.sessionDeletes {
                log.info("Session deleted topic=\(delete.topic.prefix(8)) reason=\(String(describing: delete.reason))")
                await MainActor.run {
                    activeSessions.removeAll { $0.topic == delete.topic }
                }
            }
        }
    }

    private func prettyJSON(_ value: AnyCodable) -> String? {
        guard let data = try? JSONEncoder().encode(value) else { return nil }
        guard let obj = try? JSONSerialization.jsonObject(with: data),
              let pretty = try? JSONSerialization.data(withJSONObject: obj, options: .prettyPrinted) else {
            return String(data: data, encoding: .utf8)
        }
        return String(data: pretty, encoding: .utf8)
    }
}
