import Foundation
import secp256k1
import os

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "Eip712Signer")

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

    // MARK: - RawDigest32

    /// A raw 32-byte digest wrapper that bypasses the SHA256 hashing
    /// applied by `signature(for: Data)`. Ethereum signing requires ECDSA
    /// over the raw keccak256 hash — not SHA256(keccak256).
    private struct RawDigest32: Digest {
        let bytes: (UInt64, UInt64, UInt64, UInt64)

        init(_ data: Data) {
            precondition(data.count == 32, "RawDigest32 requires exactly 32 bytes")
            let b = Array(data)
            let first  = b[0..<8].withUnsafeBytes   { $0.load(as: UInt64.self) }
            let second = b[8..<16].withUnsafeBytes  { $0.load(as: UInt64.self) }
            let third  = b[16..<24].withUnsafeBytes { $0.load(as: UInt64.self) }
            let fourth = b[24..<32].withUnsafeBytes { $0.load(as: UInt64.self) }
            self.bytes = (first, second, third, fourth)
        }

        static var byteCount: Int {
            get { 32 }
            set { fatalError("Cannot set byteCount") }
        }

        func withUnsafeBytes<R>(_ body: (UnsafeRawBufferPointer) throws -> R) rethrows -> R {
            try Swift.withUnsafeBytes(of: bytes) {
                let ptr = UnsafeRawBufferPointer(start: $0.baseAddress, count: Self.byteCount)
                return try body(ptr)
            }
        }

        func hash(into hasher: inout Hasher) {
            withUnsafeBytes { hasher.combine(bytes: $0) }
        }

        static func == (lhs: RawDigest32, rhs: RawDigest32) -> Bool {
            lhs.bytes.0 == rhs.bytes.0 && lhs.bytes.1 == rhs.bytes.1 &&
            lhs.bytes.2 == rhs.bytes.2 && lhs.bytes.3 == rhs.bytes.3
        }

        var description: String {
            var array = [UInt8]()
            withUnsafeBytes { array.append(contentsOf: $0) }
            return "RawDigest32: \(array.map { String(format: "%02x", $0) }.joined())"
        }

        func makeIterator() -> Array<UInt8>.Iterator {
            withUnsafeBytes { Array($0).makeIterator() }
        }
    }

    /// Signs an Ethereum personal message (EIP-191) and returns a 0x-prefixed hex signature (r||s||v)
    ///
    /// Prepends the standard prefix: `\x19Ethereum Signed Message:\n{len}` before hashing.
    static func signPersonalMessage(_ message: Data, privateKeyHex: String) throws -> String {
        let prefix = "\u{19}Ethereum Signed Message:\n\(message.count)"
        var input = Data(prefix.utf8)
        input.append(message)
        let hash = KeyManager_keccak256(input)
        return try performSignature(signingHash: hash, privateKeyHex: privateKeyHex)
    }

    /// Signs an EIP-712 typed data message and returns a 0x-prefixed hex signature (r||s||v)
    static func sign(typedDataJson: String, privateKeyHex: String) throws -> String {
        log.info("EIP-712 signer starting payload bytes=\(typedDataJson.utf8.count)")
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
        log.info("EIP-712 signer parsed primaryType=\(primaryType) domainFields=\(domain.keys.sorted().joined(separator: ","))")

        guard !primaryType.isEmpty else {
            throw Error.invalidTypedDataJson("missing primaryType")
        }

        // Ensure EIP712Domain type exists
        var typesDef = types
        if typesDef["EIP712Domain"] == nil {
            typesDef["EIP712Domain"] = deriveEIP712Domain(from: domain)
            log.info("EIP-712 signer derived EIP712Domain type from domain object")
        } else {
            log.info("EIP-712 signer using provided EIP712Domain type")
        }

        // Compute domain separator hash
        let domainHash = try hashStruct("EIP712Domain", domain, typesDef)
        log.info("EIP-712 signer computed domain hash \(domainHash.hexString.prefix(18))...")

        // Compute message hash
        let messageHash = try hashStruct(primaryType, message, typesDef)
        log.info("EIP-712 signer computed message hash \(messageHash.hexString.prefix(18))...")

        // Construct signing hash: keccak256(0x19 || 0x01 || domainSeparatorHash || messageHash)
        var signingInput = Data([0x19, 0x01])
        signingInput.append(domainHash)
        signingInput.append(messageHash)
        let signingHash = KeyManager_keccak256(signingInput)
        log.info("EIP-712 signer computed final signing hash \(signingHash.hexString.prefix(18))...")

        // Sign with secp256k1
        let signature = try performSignature(signingHash: signingHash, privateKeyHex: privateKeyHex)
        log.info("EIP-712 signer produced signature \(signature.prefix(20))...")
        return signature
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
        case "verifyingContract": return "address"
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
            return encodeUintValue(value, isSigned: false)
        case let t where t.hasPrefix("int"):
            return encodeUintValue(value, isSigned: true)

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

    // MARK: - Numeric Encoding (uint256 / int256)

    /// Encodes any JSON value as a 32-byte big-endian integer.
    /// Handles decimal strings, hex strings, NSNumber, and Int values
    /// up to the full uint256 / int256 range.
    private static func encodeUintValue(_ value: Any?, isSigned: Bool) -> Data {
        if let str = value as? String {
            return encodeNumericString(str, isSigned: isSigned)
        }
        if let num = value as? NSNumber {
            return encodeInt64(num.int64Value, isSigned: isSigned)
        }
        if let num = value as? Int {
            return encodeInt64(Int64(num), isSigned: isSigned)
        }
        return Data(repeating: 0, count: 32)
    }

    private static func encodeNumericString(_ str: String, isSigned: Bool) -> Data {
        let cleaned = str.trimmingCharacters(in: .whitespaces)

        if cleaned.hasPrefix("0x") || cleaned.hasPrefix("0X") {
            let hex = String(cleaned.dropFirst(2))
            guard let data = Data(hexString: hex) else {
                return Data(repeating: 0, count: 32)
            }
            return padLeft(data, to: 32)
        }

        if isSigned && cleaned.hasPrefix("-") {
            let positiveBytes = decimalStringToBytes(String(cleaned.dropFirst()))
            return twosComplement256(positiveBytes)
        }

        let bytes = decimalStringToBytes(cleaned)
        return padLeft(Data(bytes), to: 32)
    }

    /// Encodes an Int64 as 32-byte big-endian with proper sign extension.
    private static func encodeInt64(_ val: Int64, isSigned: Bool) -> Data {
        var result: Data
        if isSigned && val < 0 {
            // Sign-extend: fill all 32 bytes with 0xFF, then overwrite last 8
            result = Data(repeating: 0xFF, count: 32)
        } else {
            result = Data(repeating: 0, count: 32)
        }
        let unsigned = UInt64(bitPattern: val)
        for i in 0..<8 {
            result[24 + i] = UInt8((unsigned >> (56 - i * 8)) & 0xFF)
        }
        return result
    }

    /// Converts a decimal string of arbitrary length to big-endian bytes.
    /// Supports the full uint256 range (up to 78 decimal digits).
    private static func decimalStringToBytes(_ str: String) -> [UInt8] {
        guard !str.isEmpty else { return [0] }
        var result: [UInt8] = [0]

        for char in str {
            guard let ascii = char.asciiValue, ascii >= 48, ascii <= 57 else { continue }
            let digit = Int(ascii) - 48

            // Multiply result by 10
            var carry = 0
            for i in stride(from: result.count - 1, through: 0, by: -1) {
                let product = Int(result[i]) * 10 + carry
                result[i] = UInt8(product & 0xFF)
                carry = product >> 8
            }
            while carry > 0 {
                result.insert(UInt8(carry & 0xFF), at: 0)
                carry >>= 8
            }

            // Add digit
            var addCarry = digit
            for i in stride(from: result.count - 1, through: 0, by: -1) {
                let sum = Int(result[i]) + addCarry
                result[i] = UInt8(sum & 0xFF)
                addCarry = sum >> 8
            }
            while addCarry > 0 {
                result.insert(UInt8(addCarry & 0xFF), at: 0)
                addCarry >>= 8
            }
        }

        return result
    }

    /// Computes 256-bit two's complement of a positive big-endian byte array.
    private static func twosComplement256(_ positive: [UInt8]) -> Data {
        // Pad positive value into 32 bytes
        var bytes = [UInt8](repeating: 0, count: 32)
        let offset = max(0, 32 - positive.count)
        let srcStart = max(0, positive.count - 32)
        for i in 0..<min(positive.count, 32) {
            bytes[offset + i] = positive[srcStart + i]
        }

        // Invert all bits
        for i in 0..<32 { bytes[i] = ~bytes[i] }

        // Add 1
        var carry: UInt16 = 1
        for i in stride(from: 31, through: 0, by: -1) {
            let sum = UInt16(bytes[i]) + carry
            bytes[i] = UInt8(sum & 0xFF)
            carry = sum >> 8
        }

        return Data(bytes)
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

        // Use Recovery.PrivateKey with RawDigest32 to:
        // 1. Bypass SHA256 hashing (sign the raw keccak256 hash directly)
        // 2. Get the recovery ID from the signature (no brute-force needed)
        let recoveryKey = try secp256k1.Recovery.PrivateKey(
            dataRepresentation: keyData, format: .uncompressed
        )
        let digest = RawDigest32(signingHash)
        let recoverableSignature = try recoveryKey.signature(for: digest)

        // Use compactRepresentation for properly serialized r(32) || s(32) in big-endian
        let compact = try recoverableSignature.compactRepresentation
        let sigBytes = compact.signature
        guard sigBytes.count == 64 else {
            throw Error.signingFailed("unexpected compact signature length \(sigBytes.count)")
        }

        let r = sigBytes[sigBytes.startIndex ..< sigBytes.startIndex + 32]
        let s = sigBytes[sigBytes.startIndex + 32 ..< sigBytes.startIndex + 64]
        let v = UInt8(27) + UInt8(compact.recoveryId)

        let rHex = r.map { String(format: "%02x", $0) }.joined()
        let sHex = s.map { String(format: "%02x", $0) }.joined()
        let vHex = String(format: "%02x", v)

        return "0x\(rHex)\(sHex)\(vHex)"
    }
}
