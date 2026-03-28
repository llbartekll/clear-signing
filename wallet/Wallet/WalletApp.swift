import SwiftUI

@main
struct WalletApp: App {
    @StateObject private var viewModel: WalletViewModel

    init() {
        let metadataProvider = WalletMetadataProvider.live()
        _viewModel = StateObject(wrappedValue: WalletViewModel(metadataProvider: metadataProvider))
    }

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: viewModel)
        }
    }
}
