import Foundation
import secp256k1

enum Eip712Signer {
    enum Error: LocalizedError {
        case invalidTypedDataJson(String)
        case invalidPrivateKey
        case signingFailed(String)
        case missingDomainFields
        case invalidDomainStructure

        var errorDescription: String? {
            switch self {
            case .invalidTypedDataJson(let reason):
                return "Invalid EIP-712 typed data JSON: \(reason)"
            case .invalidPrivateKey:
                return "Invalid private key format"
            case .signingFailed(let reason):
                return "EIP-712 signing failed: \(reason)"
            case .missingDomainFields:
                return "EIP712Domain type definition required or derivable from domain"
            case .invalidDomainStructure:
                return "Domain structure is invalid"
            }
        }
    }

    /// Signs an EIP-712 typed data message and returns a 0x-prefixed hex signature (r||s||v)
    static func sign(typedDataJson: String, privateKeyHex: String) throws -> String {
        guard let jsonData = typedDataJson.data(using: .utf8) else {
            throw Error.invalidTypedDataJson("not valid UTF-8")
        }

        let json = try JSONSerialization.jsonObject(with: jsonData) as? [String: Any]
        guard let json else {
            throw Error.invalidTypedDataJson("not a JSON object")
        }

        let types = json["types"] as? [String: [[String: String]]] ?? [:]
        let domain = json["domain"] as? [String: Any] ?? [:]
        let message = json["message"] as? [String: Any] ?? [:]
        let primaryType = json["primaryType"] as? String ?? ""

        guard !primaryType.isEmpty else {
            throw Error.invalidTypedDataJson("missing primaryType")
        }

        // Ensure EIP712Domain type exists
        var typesDef = types
        if typesDef["EIP712Domain"] == nil {
            typesDef["EIP712Domain"] = deriveEIP712Domain(from: domain)
        }

        // Compute domain separator hash
        let domainHash = try hashStruct("EIP712Domain", domain, typesDef)

        // Compute message hash
        let messageHash = try hashStruct(primaryType, message, typesDef)

        // Construct signing hash: keccak256(0x19 || 0x01 || domainSeparatorHash || messageHash)
        var signingInput = Data([0x19, 0x01])
        signingInput.append(domainHash)
        signingInput.append(messageHash)
        let signingHash = KeyManager_keccak256(signingInput)

        // Sign with secp256k1
        return try performSignature(signingHash: signingHash, privateKeyHex: privateKeyHex)
    }

    // MARK: - EIP-712 Algorithm Functions

    private static func deriveEIP712Domain(from domain: [String: Any]) -> [[String: String]] {
        var fields: [[String: String]] = []
        let canonicalOrder = ["name", "version", "chainId", "verifyingContract", "salt"]

        for key in canonicalOrder {
            if domain[key] != nil {
                fields.append(["name": key, "type": typeForDomainField(key)])
            }
        }

        return fields
    }

    private static func typeForDomainField(_ field: String) -> String {
        switch field {
        case "chainId": return "uint256"
        case "salt": return "bytes32"
        default: return "string"
        }
    }

    /// Builds "TypeName(type1 field1,type2 field2,...)" with recursive type collection
    private static func encodeType(_ typeName: String, _ types: [String: [[String: String]]]) -> String {
        guard let typeFields = types[typeName] else {
            return "\(typeName)()"
        }

        let fieldSignatures = typeFields.map { field in
            let name = field["name"] ?? ""
            let type = field["type"] ?? ""
            return "\(type) \(name)"
        }.joined(separator: ",")

        let base = "\(typeName)(\(fieldSignatures))"

        // Recursively collect referenced struct types
        var referencedTypes = Set<String>()
        for field in typeFields {
            if let type = field["type"] {
                collectReferencedTypes(type, types, &referencedTypes)
            }
        }

        // Sort and append
        let sorted = referencedTypes.sorted()
        var result = base
        for refType in sorted {
            result += encodeType(refType, types)
        }

        return result
    }

    private static func collectReferencedTypes(_ fieldType: String, _ types: [String: [[String: String]]],
                                               _ collected: inout Set<String>) {
        // Extract base type (handle array notation like "Type[]" or "Type[5]")
        let baseType: String
        if fieldType.contains("[") {
            baseType = String(fieldType.prefix(while: { $0 != "[" }))
        } else {
            baseType = fieldType
        }

        // Skip primitives
        if isPrimitive(baseType) {
            return
        }

        // If it's a struct type we haven't seen, mark it for collection
        if types[baseType] != nil && !collected.contains(baseType) {
            collected.insert(baseType)
        }
    }

    private static func isPrimitive(_ type: String) -> Bool {
        let primitives = [
            "bool", "string", "bytes", "address",
            "uint", "uint8", "uint16", "uint32", "uint64", "uint128", "uint256",
            "int", "int8", "int16", "int32", "int64", "int128", "int256",
            "bytes1", "bytes2", "bytes4", "bytes8", "bytes16", "bytes32"
        ]
        return primitives.contains(type) ||
               type.hasPrefix("uint") || type.hasPrefix("int") ||
               type.hasPrefix("bytes")
    }

    /// keccak256(keccak256(encodeType) || encodeData(...))
    private static func hashStruct(_ typeName: String, _ data: [String: Any],
                                   _ types: [String: [[String: String]]]) throws -> Data {
        let typeEncodingStr = encodeType(typeName, types)
        let typeHash = KeyManager_keccak256(Data(typeEncodingStr.utf8))

        let dataEncoding = try encodeData(typeName, data, types)

        var structInput = typeHash
        structInput.append(dataEncoding)
        return KeyManager_keccak256(structInput)
    }

    /// Concatenates 32-byte-encoded values for each field
    private static func encodeData(_ typeName: String, _ data: [String: Any],
                                   _ types: [String: [[String: String]]]) throws -> Data {
        guard let typeFields = types[typeName] else {
            return Data()
        }

        var result = Data()
        for field in typeFields {
            let fieldName = field["name"] ?? ""
            let fieldType = field["type"] ?? ""
            let fieldValue = data[fieldName]

            let encoded = try encodeValue(fieldValue, fieldType, types)
            result.append(encoded)
        }

        return result
    }

    /// Per-type value encoding to 32 bytes
    private static func encodeValue(_ value: Any?, _ type: String,
                                    _ types: [String: [[String: String]]]) throws -> Data {
        // Handle array types
        if type.contains("[") {
            return try encodeArray(value, type, types)
        }

        // Handle struct types
        if types[type] != nil {
            let dataDict = value as? [String: Any] ?? [:]
            let hash = try hashStruct(type, dataDict, types)
            return padLeft(hash, to: 32)
        }

        // Handle primitives
        switch type {
        case "string":
            if let str = value as? String {
                return padLeft(KeyManager_keccak256(Data(str.utf8)), to: 32)
            }
            return Data(repeating: 0, count: 32)

        case "bytes":
            if let hexStr = value as? String {
                let cleaned = hexStr.hasPrefix("0x") ? String(hexStr.dropFirst(2)) : hexStr
                if let data = Data(hexString: cleaned) {
                    return padLeft(KeyManager_keccak256(data), to: 32)
                }
            }
            return Data(repeating: 0, count: 32)

        case let t where t.hasPrefix("bytes") && t.count > 5:
            // Fixed-size bytes (bytesN)
            let sizeStr = String(t.dropFirst(5))
            if let size = Int(sizeStr), size > 0 && size <= 32 {
                if let hexStr = value as? String {
                    let cleaned = hexStr.hasPrefix("0x") ? String(hexStr.dropFirst(2)) : hexStr
                    if let data = Data(hexString: cleaned) {
                        return padRight(data, to: 32)
                    }
                }
            }
            return Data(repeating: 0, count: 32)

        case "address":
            if let addr = value as? String {
                let cleaned = addr.hasPrefix("0x") ? String(addr.dropFirst(2)) : addr
                if let data = Data(hexString: cleaned) {
                    return padLeft(data, to: 32)
                }
            }
            return Data(repeating: 0, count: 32)

        case "bool":
            let boolValue = (value as? Bool) ?? false
            var result = Data(repeating: 0, count: 31)
            result.append(boolValue ? 1 : 0)
            return result

        case let t where t.hasPrefix("uint"):
            return try encodeUint(value, t, isSigned: false)
        case let t where t.hasPrefix("int"):
            return try encodeUint(value, t, isSigned: true)

        default:
            return Data(repeating: 0, count: 32)
        }
    }

    private static func encodeArray(_ value: Any?, _ type: String,
                                     _ types: [String: [[String: String]]]) throws -> Data {
        // Extract element type and check if fixed-size
        guard let bracketIdx = type.firstIndex(of: "[") else {
            return Data()
        }

        let elementType = String(type[..<bracketIdx])
        let arraySuffix = String(type[bracketIdx...])
        let isFixed = !arraySuffix.contains("[]")

        // Get array elements
        var elements: [Any] = []
        if let arr = value as? [Any] {
            elements = arr
        } else if let arr = value as? [String] {
            elements = arr
        } else if let arr = value as? [Int] {
            elements = arr
        } else if let arr = value as? [Double] {
            elements = arr
        }

        // For dynamic arrays, hash the encoded elements
        if !isFixed {
            var encoded = Data()
            for elem in elements {
                let elemEncoded = try encodeValue(elem, elementType, types)
                encoded.append(elemEncoded)
            }
            return padLeft(KeyManager_keccak256(encoded), to: 32)
        }

        // For fixed arrays, return concatenated elements
        var result = Data()
        for elem in elements {
            let encoded = try encodeValue(elem, elementType, types)
            result.append(encoded)
        }
        return result
    }

    private static func encodeUint(_ value: Any?, _ type: String, isSigned: Bool) throws -> Data {
        let bits: Int
        if type.count > (isSigned ? 3 : 4) {
            let numStr = isSigned ? String(type.dropFirst(3)) : String(type.dropFirst(4))
            bits = Int(numStr) ?? 256
        } else {
            bits = 256
        }

        var intValue: Int64 = 0

        if let num = value as? NSNumber {
            intValue = num.int64Value
        } else if let num = value as? Int {
            intValue = Int64(num)
        } else if let str = value as? String {
            let cleaned = str.hasPrefix("0x") ? String(str.dropFirst(2)) : str
            if cleaned.hasPrefix("-") {
                intValue = Int64(cleaned) ?? 0
            } else if let parsed = Int64(cleaned) {
                intValue = parsed
            } else if let bigInt = parseBigInt(cleaned) {
                intValue = bigInt
            }
        }

        var result = Data(repeating: 0, count: 32)

        if isSigned && intValue < 0 {
            // Two's complement for negative values
            let unsigned = UInt64(bitPattern: intValue)
            for i in 0..<8 {
                result[24 + i] = UInt8((unsigned >> (56 - i * 8)) & 0xFF)
            }
        } else {
            // Positive value: big-endian encoding
            let unsigned = UInt64(bitPattern: intValue)
            for i in 0..<8 {
                result[24 + i] = UInt8((unsigned >> (56 - i * 8)) & 0xFF)
            }
        }

        return result
    }

    private static func parseBigInt(_ hex: String) -> Int64? {
        // Simple big integer parsing for values that fit in Int64
        if hex.count <= 16 {  // 16 hex digits = 64 bits
            return Int64(hex, radix: 16)
        }
        return nil
    }

    // MARK: - Padding Helpers

    private static func padLeft(_ data: Data, to size: Int) -> Data {
        if data.count >= size {
            return Data(data.suffix(size))
        }
        var result = Data(repeating: 0, count: size - data.count)
        result.append(data)
        return result
    }

    private static func padRight(_ data: Data, to size: Int) -> Data {
        if data.count >= size {
            return Data(data.prefix(size))
        }
        var result = data
        result.append(Data(repeating: 0, count: size - data.count))
        return result
    }

    // MARK: - secp256k1 Signing

    private static func performSignature(signingHash: Data, privateKeyHex: String) throws -> String {
        let cleaned = privateKeyHex.hasPrefix("0x") ? String(privateKeyHex.dropFirst(2)) : privateKeyHex
        guard let keyData = Data(hexString: cleaned) else {
            throw Error.invalidPrivateKey
        }

        let privateKey = try secp256k1.Signing.PrivateKey(dataRepresentation: keyData, format: .uncompressed)
        let publicKey = privateKey.publicKey

        // Sign the hash using the Signing API
        let signature = try privateKey.signature(for: signingHash)
        let compactSig = try signature.compactRepresentation

        // Extract r and s (each 32 bytes)
        let r = compactSig.prefix(32)
        let s = compactSig.suffix(32)

        // Compute recovery ID by trying all 4 possibilities
        let recoveryId = try computeRecoveryId(signingHash: signingHash, publicKey: publicKey,
                                                r: r, s: s)
        let v = UInt8(27 + recoveryId)

        let rHex = r.map { String(format: "%02x", $0) }.joined()
        let sHex = s.map { String(format: "%02x", $0) }.joined()
        let vHex = String(format: "%02x", v)

        return "0x\(rHex)\(sHex)\(vHex)"
    }

    private static func computeRecoveryId(signingHash: Data, publicKey: secp256k1.Signing.PublicKey,
                                           r: Data, s: Data) throws -> UInt8 {
        let publicKeyData = publicKey.dataRepresentation

        // Try each recovery ID (0, 1, 2, 3) and see which one recovers our public key
        for recoveryId: UInt8 in 0..<4 {
            do {
                // Reconstruct the signature with this recovery ID
                var sigBytes = Data(r + s)
                sigBytes.append(recoveryId)

                // Try to recover the public key
                let recoverySignature = try secp256k1.Recovery.ECDSASignature(dataRepresentation: sigBytes)
                let recoveredKey = try secp256k1.Recovery.PublicKey(signingHash,
                                                                     signature: recoverySignature,
                                                                     format: .uncompressed)
                let recoveredData = recoveredKey.dataRepresentation

                // Check if this matches our public key
                if recoveredData == publicKeyData {
                    return recoveryId
                }
            } catch {
                // This recovery ID didn't work, try the next one
                continue
            }
        }

        // Fallback: if recovery fails, default to recovery ID 0
        // This shouldn't happen with a valid signature
        return 0
    }
}
