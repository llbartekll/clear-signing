import Foundation

enum CacheLookup<Value> {
    case value(Value)
    case negative
    case missing
}

extension CacheLookup: Equatable where Value: Equatable {}

struct CachedResolution<Value: Codable>: Codable {
    let value: Value?
    let storedAt: Date
    let expiresAt: Date

    func isExpired(at date: Date) -> Bool {
        expiresAt <= date
    }
}

final class InMemoryResolutionCache {
    private let lock = NSLock()
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()
    private var storage: [String: Data] = [:]

    func lookup<Value: Codable>(_ key: String, as type: Value.Type, now: Date) -> CacheLookup<Value> {
        lock.lock()
        defer { lock.unlock() }

        guard let data = storage[key] else {
            return .missing
        }

        guard let record = try? decoder.decode(CachedResolution<Value>.self, from: data) else {
            storage.removeValue(forKey: key)
            return .missing
        }

        if record.isExpired(at: now) {
            storage.removeValue(forKey: key)
            return .missing
        }

        if let value = record.value {
            return .value(value)
        }

        return .negative
    }

    func store<Value: Codable>(_ value: Value?, key: String, ttl: TimeInterval, now: Date) {
        let record = CachedResolution(
            value: value,
            storedAt: now,
            expiresAt: now.addingTimeInterval(ttl)
        )

        guard let data = try? encoder.encode(record) else {
            return
        }

        lock.lock()
        storage[key] = data
        lock.unlock()
    }
}

final class PersistentResolutionCache {
    private let userDefaults: UserDefaults
    private let namespace: String
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(userDefaults: UserDefaults, namespace: String = "wallet.metadata") {
        self.userDefaults = userDefaults
        self.namespace = namespace
    }

    func lookup<Value: Codable>(_ key: String, as type: Value.Type, now: Date) -> CacheLookup<Value> {
        let namespacedKey = namespaced(key)
        guard let data = userDefaults.data(forKey: namespacedKey) else {
            return .missing
        }

        guard let record = try? decoder.decode(CachedResolution<Value>.self, from: data) else {
            userDefaults.removeObject(forKey: namespacedKey)
            return .missing
        }

        if record.isExpired(at: now) {
            userDefaults.removeObject(forKey: namespacedKey)
            return .missing
        }

        if let value = record.value {
            return .value(value)
        }

        return .negative
    }

    func store<Value: Codable>(_ value: Value?, key: String, ttl: TimeInterval, now: Date) {
        let record = CachedResolution(
            value: value,
            storedAt: now,
            expiresAt: now.addingTimeInterval(ttl)
        )

        guard let data = try? encoder.encode(record) else {
            return
        }

        userDefaults.set(data, forKey: namespaced(key))
    }

    private func namespaced(_ key: String) -> String {
        "\(namespace).\(key)"
    }
}

struct SeedTokenStore {
    private let tokens: [String: TokenMetadata]

    init(bundle: Bundle, resourceName: String = "tokens", resourceExtension: String = "json") {
        let data = bundle.url(forResource: resourceName, withExtension: resourceExtension)
            .flatMap { try? Data(contentsOf: $0) }
        tokens = Self.decodeTokens(from: data)
    }

    init(data: Data) {
        tokens = Self.decodeTokens(from: data)
    }

    func token(chainId: UInt64, address: String) -> TokenMetadata? {
        tokens[LookupKey.token(chainId: chainId, address: address)]
    }

    private static func decodeTokens(from data: Data?) -> [String: TokenMetadata] {
        guard let data else {
            return [:]
        }

        let decoder = JSONDecoder()
        return (try? decoder.decode([String: TokenMetadata].self, from: data)) ?? [:]
    }
}

enum RemoteLookup<Value> {
    case value(Value)
    case notFound
    case unavailable
}
