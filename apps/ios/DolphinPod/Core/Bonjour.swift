import Foundation
import Network

/// Finds desktops on the local network (`_dolphinpod._tcp`) so remote requests can take the fast path.
/// Needs the Local Network permission; if it is denied the app simply uses the cloud path.
@MainActor
final class BonjourBrowser {
    struct Endpoint: Equatable { var host: String; var port: UInt16 }

    private var browser: NWBrowser?
    /// Desktop device id (from the TXT record) -> where to reach it.
    private(set) var endpoints: [String: Endpoint] = [:]
    var onChange: (() -> Void)?
    private var resolving: [String: NWConnection] = [:]

    func start() {
        guard browser == nil else { return }
        let params = NWParameters()
        params.includePeerToPeer = false
        let b = NWBrowser(for: .bonjourWithTXTRecord(type: "_dolphinpod._tcp", domain: nil), using: params)
        b.browseResultsChangedHandler = { [weak self] results, _ in
            Task { @MainActor in self?.update(results) }
        }
        b.stateUpdateHandler = { state in
            if case .failed = state { b.cancel() }
        }
        b.start(queue: .main)
        browser = b
    }

    func stop() {
        browser?.cancel()
        browser = nil
        resolving.values.forEach { $0.cancel() }
        resolving.removeAll()
    }

    private func update(_ results: Set<NWBrowser.Result>) {
        var seen = Set<String>()
        for r in results {
            guard case .bonjour(let txt) = r.metadata, let id = txt["id"] else { continue }
            seen.insert(id)
            if endpoints[id] == nil, resolving[id] == nil { resolve(id: id, endpoint: r.endpoint) }
        }
        for id in endpoints.keys where !seen.contains(id) { endpoints[id] = nil }
        onChange?()
    }

    /// Opens a throwaway connection to learn the service's address and port.
    private func resolve(id: String, endpoint: NWEndpoint) {
        let params = NWParameters.tcp
        // Prefer IPv4: link-local IPv6 addresses would need a scope the core doesn't handle.
        (params.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options)?.version = .v4
        let conn = NWConnection(to: endpoint, using: params)
        resolving[id] = conn
        conn.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                if case .hostPort(let host, let port)? = conn.currentPath?.remoteEndpoint {
                    let hostString: String
                    switch host {
                    case .ipv4(let a): hostString = "\(a)"
                    case .ipv6(let a): hostString = "\(a)"
                    case .name(let n, _): hostString = n
                    @unknown default: hostString = "\(host)"
                    }
                    Task { @MainActor in
                        self?.endpoints[id] = Endpoint(host: hostString.components(separatedBy: "%").first ?? hostString, port: port.rawValue)
                        self?.resolving[id] = nil
                        self?.onChange?()
                    }
                }
                conn.cancel()
            case .failed, .cancelled:
                Task { @MainActor in self?.resolving[id] = nil }
            default: break
            }
        }
        conn.start(queue: .main)
    }
}
