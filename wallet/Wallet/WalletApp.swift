import SwiftUI

@main
struct WalletApp: App {
    private let viewModel: WalletViewModel

    init() {
        let metadataProvider = WalletMetadataProvider.live()
        viewModel = WalletViewModel(metadataProvider: metadataProvider)
    }

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: viewModel)
        }
    }
}
