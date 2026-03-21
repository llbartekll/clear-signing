import Foundation

enum ENSCoinType {
    static func value(for chainId: UInt64) -> UInt64 {
        chainId == 1 ? 60 : (0x8000_0000 ^ chainId)
    }
}

enum UniversalResolverCall {
    static let contractAddress = "0xeEeEEEeE14D718C2B47D9923Deab1335E144EeEe"

    static func reverseCallData(address: String, coinType: UInt64) -> String? {
        guard let addressBytes = Data(hexString: address) else {
            return nil
        }

        var payload = Data()
        payload.append(functionSelector(for: "reverse(bytes,uint256)"))
        payload.append(abiEncodeUInt256(64))
        payload.append(abiEncodeUInt256(coinType))
        payload.append(abiEncodeBytes(addressBytes))
        return "0x" + payload.hexString
    }

    static func decodePrimaryName(fromHex hex: String) -> String? {
        guard let data = Data(hexString: hex), data.count >= 96 else {
            return nil
        }

        let offset = Int(readUInt64(from: data, at: 0))
        guard offset + 32 <= data.count else {
            return nil
        }

        let length = Int(readUInt64(from: data, at: offset))
        let valueStart = offset + 32
        let valueEnd = valueStart + length
        guard length > 0, valueEnd <= data.count else {
            return nil
        }

        let nameData = data.subdata(in: valueStart..<valueEnd)
        guard let name = String(data: nameData, encoding: .utf8) else {
            return nil
        }

        return normalizedString(name)
    }

    private static func functionSelector(for signature: String) -> Data {
        Data(keccak256(Data(signature.utf8)).prefix(4))
    }

    private static func abiEncodeBytes(_ value: Data) -> Data {
        var encoded = Data()
        encoded.append(abiEncodeUInt256(UInt64(value.count)))
        encoded.append(value)
        let remainder = value.count % 32
        if remainder != 0 {
            encoded.append(Data(repeating: 0, count: 32 - remainder))
        }
        return encoded
    }

    private static func abiEncodeUInt256(_ value: UInt64) -> Data {
        var encoded = Data(repeating: 0, count: 32)
        withUnsafeBytes(of: value.bigEndian) { rawBuffer in
            encoded.replaceSubrange(24..<32, with: rawBuffer)
        }
        return encoded
    }

    private static func readUInt64(from data: Data, at offset: Int) -> UInt64 {
        guard offset + 32 <= data.count else {
            return 0
        }

        let word = data.subdata(in: offset..<(offset + 32))
        return word.suffix(8).reduce(UInt64(0)) { partialResult, byte in
            (partialResult << 8) | UInt64(byte)
        }
    }
}

func normalizedAddress(_ value: String?) -> String? {
    guard let value else {
        return nil
    }

    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard trimmed.count == 42, trimmed.hasPrefix("0x") else {
        return nil
    }

    guard Data(hexString: trimmed) != nil else {
        return nil
    }

    return trimmed.lowercased()
}

func normalizedString(_ value: String?) -> String? {
    guard let value else {
        return nil
    }

    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}
