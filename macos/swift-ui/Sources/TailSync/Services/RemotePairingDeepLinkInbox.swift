import Foundation

struct RemotePairingDeepLinkInbox {
    private var pendingLink: String?

    mutating func receive(_ url: URL) -> String? {
        guard url.scheme?.lowercased() == "tailsync",
              url.host?.lowercased() == "pair",
              url.user == nil,
              url.password == nil,
              url.port == nil,
              url.query == nil,
              url.fragment == nil
        else { return nil }

        let components = url.path.split(separator: "/", omittingEmptySubsequences: true)
        guard components.count == 2,
              components[0] == "v1",
              !components[1].isEmpty,
              components[1].allSatisfy({ character in
                  character.isASCII && (character.isLetter || character.isNumber || character == "_" || character == "-")
              })
        else { return nil }

        let link = url.absoluteString
        pendingLink = link
        return link
    }

    mutating func takePending() -> String? {
        defer { pendingLink = nil }
        return pendingLink
    }
}
