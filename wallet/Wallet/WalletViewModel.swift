import Foundation
import os
import ClearSigning
import ReownWalletKit

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "WalletViewModel")

final class WalletViewModel: ObservableObject {

    // Key management
    @Published var privateKeyHex = ""
    @Published var ethereumAddress: String?
    @Published var keyError: String?

    // WalletConnect
    @Published var pairingURI = ""
    @Published var isPaired = false
    @Published var pairingError: String?

    // Sessions
    @Published var activeSessions: [Session] = []

    // Proposal
    @Published var pendingProposal: Session.Proposal?
    @Published var showProposal = false

    // Request
    @Published var pendingRequest: Request?
    @Published var displayModel: DisplayModel?
    @Published var requestError: String?
    @Published var rawRequestJSON: String?
    @Published var showRequest = false
    @Published var currentCalldataCapture: CalldataCapture?
    @Published var recentCalldataCaptures: [CalldataCapture] = []
    @Published var currentTypedDataCapture: TypedDataCapture?
    @Published var recentTypedDataCaptures: [TypedDataCapture] = []

    // QR
    @Published var showScanner = false

    private var keyManager: KeyManager?
    private let clearSigning: ClearSigningService
    private let wc = WalletConnectService.shared
    @Published var wcConfigured = false

    init(metadataProvider: DataProviderFfi) {
        clearSigning = ClearSigningService(dataProvider: metadataProvider)
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
                currentCalldataCapture = nil
                currentTypedDataCapture = nil
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

        // Auto-respond to capability queries without showing UI
        if request.method == "wallet_getCapabilities" {
            respondCapabilities(request)
            return
        }

        pendingRequest = request
        displayModel = nil
        requestError = nil
        rawRequestJSON = nil
        currentCalldataCapture = nil
        currentTypedDataCapture = nil

        let method = request.method

        if method == "eth_sendTransaction" {
            processTransaction(request)
        } else if method == "eth_signTypedData" || method == "eth_signTypedData_v4" {
            processTypedData(request)
        } else if method == "personal_sign" {
            processPersonalSign(request)
        } else {
            log.warning("Unsupported method: \(method)")
            rawRequestJSON = prettyJSON(request.params)
            requestError = "Unsupported method: \(method)"
            if method == "eth_signTypedData" || method == "eth_signTypedData_v4" {
                var capture = TypedDataCapture(request: request, rawParamsJson: rawRequestJSON)
                capture.outcome = .unsupportedMethod
                capture.notes.append("Unsupported typed-data RPC method")
                setCurrentTypedDataCapture(capture)
            }
        }
        showRequest = true
    }

    private func respondCapabilities(_ request: Request) {
        Task {
            // Build capabilities keyed by hex chain ID from active sessions
            var capabilities: [String: [String: AnyCodable]] = [:]
            let sessions = await wc.sessions
            for session in sessions {
                guard let chains = session.namespaces["eip155"]?.chains else { continue }
                for chain in chains {
                    let hexChainId = "0x" + String(Int(chain.reference) ?? 0, radix: 16)
                    capabilities[hexChainId] = [:]
                }
            }
            if capabilities.isEmpty {
                capabilities["0x1"] = [:]
            }

            do {
                try await wc.respondSuccess(request, value: AnyCodable(capabilities))
                log.info("Responded to wallet_getCapabilities with \(capabilities.count) chain(s)")
            } catch {
                log.error("Failed to respond to wallet_getCapabilities: \(error)")
            }
        }
    }

    func rejectRequest() {
        guard let request = pendingRequest else { return }
        Task {
            try? await wc.rejectRequest(request)
            await MainActor.run {
                if request.method == "eth_sendTransaction" {
                    self.updateCurrentCalldataCapture { capture in
                        capture.outcome = .rejected
                        capture.notes.append("User rejected the request")
                    }
                }
                if request.method == "eth_signTypedData" || request.method == "eth_signTypedData_v4" {
                    self.updateCurrentTypedDataCapture { capture in
                        capture.outcome = .rejected
                        capture.notes.append("User rejected the request")
                    }
                }
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
                    await MainActor.run {
                        self.updateCurrentCalldataCapture { capture in
                            capture.expectedAddress = expectedAddress
                            capture.outcome = .signingSucceeded
                            capture.notes.append("Transaction signed and sent successfully")
                            capture.notes.append("Transaction hash \(txHash)")
                        }
                    }
                    responseValue = AnyCodable(txHash)

                case "eth_signTypedData", "eth_signTypedData_v4":
                    let signature = try signer.signTypedData(
                        request: request,
                        privateKeyHex: keyManager.privateKeyHex,
                        expectedAddress: expectedAddress
                    )
                    await MainActor.run {
                        self.updateCurrentTypedDataCapture { capture in
                            capture.expectedAddress = expectedAddress
                            capture.outcome = .signingSucceeded
                            capture.notes.append("Signature produced successfully")
                        }
                    }
                    responseValue = AnyCodable(signature)

                case "personal_sign":
                    log.info("Signing personal_sign for address=\(expectedAddress)")
                    let signature = try signer.signPersonalMessage(
                        request: request,
                        privateKeyHex: keyManager.privateKeyHex,
                        expectedAddress: expectedAddress
                    )
                    log.info("personal_sign signature: \(signature.prefix(20))...")
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
                log.error("approveRequest failed: method=\(request.method) error=\(error)")
                let rpcError: JSONRPCError
                switch error {
                case EvmSigningService.SigningError.addressMismatch(_, _):
                    rpcError = JSONRPCError(code: 4001, message: "User rejected")
                case EvmSigningService.SigningError.invalidParams(_):
                    log.error("invalidParams error sent to dApp")
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
                    if request.method == "eth_sendTransaction" {
                        self.updateCurrentCalldataCapture { capture in
                            capture.outcome = .signingFailed
                            capture.signingError = error.localizedDescription
                            capture.expectedAddress = keyManager.ethereumAddress
                            capture.notes.append("Transaction signing or send failed")
                        }
                    }
                    if request.method == "eth_signTypedData" || request.method == "eth_signTypedData_v4" {
                        self.updateCurrentTypedDataCapture { capture in
                            capture.outcome = .signingFailed
                            capture.signerError = error.localizedDescription
                            capture.expectedAddress = keyManager.ethereumAddress
                        }
                    }
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
        let rawParams = prettyJSON(request.params)
        var capture = CalldataCapture(request: request, rawParamsJson: rawParams)
        setCurrentCalldataCapture(capture)

        guard let paramsArray = try? request.params.get([TransactionParams].self),
              let tx = paramsArray.first else {
            log.error("Could not parse transaction params")
            requestError = "Could not parse transaction params"
            rawRequestJSON = rawParams
            updateCurrentCalldataCapture { current in
                current.outcome = .paramsExtractionFailed
                current.signingError = "Could not parse transaction params"
                current.notes.append("Transaction params parsing failed")
            }
            return
        }

        rawRequestJSON = rawParams

        guard let to = tx.to else {
            requestError = "Transaction has no recipient (contract creation)"
            updateCurrentCalldataCapture { current in
                current.outcome = .paramsExtractionFailed
                current.signingError = "Transaction has no recipient (contract creation)"
                current.notes.append("Contract creation transaction is unsupported for clear signing")
            }
            return
        }

        let chainRef = request.chainId
        let chainId = UInt64(chainRef.reference) ?? 1

        let calldata = tx.data ?? tx.input ?? "0x"
        capture.to = to
        capture.from = tx.from
        capture.value = tx.value
        capture.calldata = calldata
        capture.outcome = .paramsExtracted
        capture.notes.append("Parsed transaction params for \(to)")
        setCurrentCalldataCapture(capture)
        log.info("Processing tx: to=\(to) chainId=\(chainId) calldata=\(calldata.prefix(10))...")

        Task {
            let result = await clearSigning.formatCalldataDetailed(
                chainId: chainId,
                to: to,
                calldata: calldata,
                value: tx.value,
                from: tx.from
            )
            await MainActor.run {
                if let model = result.model {
                    log.info("Clear signing OK: intent=\(model.intent) entries=\(model.entries.count)")
                    displayModel = model
                    self.updateCurrentCalldataCapture { current in
                        current.applyClearSigningSuccess(result)
                        if result.usedImplementationAddress, let implementationAddress = result.implementationAddress {
                            current.notes.append("Matched implementation address \(implementationAddress)")
                        }
                    }
                } else if let error = result.error {
                    log.error("Clear signing failed: \(error)")
                    requestError = error.localizedDescription
                    self.updateCurrentCalldataCapture { current in
                        current.applyClearSigningFailure(result)
                        if let stage = result.failedStage {
                            current.notes.append("Clear signing failed during \(stage.rawValue)")
                        }
                        if result.usedImplementationAddress, let implementationAddress = result.implementationAddress {
                            current.notes.append("Resolved proxy implementation \(implementationAddress)")
                        }
                    }
                }
            }
        }
    }

    private func processPersonalSign(_ request: Request) {
        do {
            let payload = try EvmSigningService.shared.extractPersonalSignPayload(from: request.params)
            let decoded = String(data: payload.message, encoding: .utf8)
            let hexStr = "0x" + payload.message.map { String(format: "%02x", $0) }.joined()

            if let text = decoded {
                rawRequestJSON = "Message:\n\(text)\n\nHex:\n\(hexStr)"
            } else {
                rawRequestJSON = "Hex:\n\(hexStr)"
            }
            log.info("Processing personal_sign: \(hexStr.prefix(20))...")
        } catch {
            log.error("Could not parse personal_sign params: \(error)")
            requestError = error.localizedDescription
            rawRequestJSON = prettyJSON(request.params)
        }
    }

    private func processTypedData(_ request: Request) {
        let rawParams = prettyJSON(request.params)
        var capture = TypedDataCapture(request: request, rawParamsJson: rawParams)
        setCurrentTypedDataCapture(capture)
        do {
            let payload = try EvmSigningService.shared.extractTypedDataPayload(from: request.params)
            let typedDataJson = payload.json
            rawRequestJSON = typedDataJson
            capture.requestedAddress = payload.address
            capture.typedDataJson = typedDataJson
            capture.summary = TypedDataSummary.from(json: typedDataJson)
            capture.outcome = .payloadExtracted
            if let primaryType = capture.summary?.primaryType {
                capture.notes.append("Extracted typed data for primaryType \(primaryType)")
            }
            setCurrentTypedDataCapture(capture)
            log.info(
                "Processing typed data primaryType=\(capture.summary?.primaryType ?? "unknown") verifyingContract=\(capture.summary?.verifyingContract ?? "nil")"
            )

            Task {
                let result = await clearSigning.formatTypedDataDetailed(typedDataJson: typedDataJson)
                await MainActor.run {
                    if let model = result.model {
                        displayModel = model
                        self.updateCurrentTypedDataCapture { current in
                            current.applyClearSigningSuccess(model, descriptorOwners: result.descriptorOwners)
                        }
                    } else if let error = result.error {
                        requestError = error.localizedDescription
                        self.updateCurrentTypedDataCapture { current in
                            current.applyClearSigningFailure(
                                error: error.localizedDescription,
                                descriptorOwners: result.descriptorOwners
                            )
                            if let stage = result.failedStage {
                                current.notes.append("Clear signing failed during \(stage)")
                            }
                        }
                    }
                }
            }
        } catch {
            requestError = error.localizedDescription
            rawRequestJSON = prettyJSON(request.params)
            updateCurrentTypedDataCapture { current in
                current.outcome = .payloadExtractionFailed
                current.signerError = error.localizedDescription
                current.notes.append("Typed data payload extraction failed")
            }
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

    private func setCurrentTypedDataCapture(_ capture: TypedDataCapture) {
        currentTypedDataCapture = capture
        mergeTypedDataCapture(capture)
    }

    private func setCurrentCalldataCapture(_ capture: CalldataCapture) {
        currentCalldataCapture = capture
        mergeCalldataCapture(capture)
    }

    private func updateCurrentTypedDataCapture(_ update: (inout TypedDataCapture) -> Void) {
        guard var capture = currentTypedDataCapture else { return }
        update(&capture)
        currentTypedDataCapture = capture
        mergeTypedDataCapture(capture)
    }

    private func updateCurrentCalldataCapture(_ update: (inout CalldataCapture) -> Void) {
        guard var capture = currentCalldataCapture else { return }
        update(&capture)
        currentCalldataCapture = capture
        mergeCalldataCapture(capture)
    }

    private func mergeTypedDataCapture(_ capture: TypedDataCapture) {
        if let index = recentTypedDataCaptures.firstIndex(where: { $0.id == capture.id }) {
            recentTypedDataCaptures[index] = capture
        } else {
            recentTypedDataCaptures.insert(capture, at: 0)
            if recentTypedDataCaptures.count > 25 {
                recentTypedDataCaptures.removeLast(recentTypedDataCaptures.count - 25)
            }
        }
    }

    private func mergeCalldataCapture(_ capture: CalldataCapture) {
        if let index = recentCalldataCaptures.firstIndex(where: { $0.id == capture.id }) {
            recentCalldataCaptures[index] = capture
        } else {
            recentCalldataCaptures.insert(capture, at: 0)
            if recentCalldataCaptures.count > 25 {
                recentCalldataCaptures.removeLast(recentCalldataCaptures.count - 25)
            }
        }
    }
}
