import SwiftUI
import Erc7730

struct ContentView: View {
    @State private var status = "Tap to run smoke test"

    var body: some View {
        VStack(spacing: 16) {
            Text("Wallet")
                .font(.largeTitle)

            Text(status)
                .font(.footnote)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)

            Button("Run smoke test") {
                runSmokeTest()
            }
        }
        .padding()
    }

    private func runSmokeTest() {
        let descriptorJson = #"""
        {
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7" }
                    ]
                }
            },
            "metadata": {
                "owner": "wallet-smoke-test",
                "contractName": "Tether USD",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "transfer(address,uint256)": {
                        "intent": "Transfer tokens",
                        "fields": [
                            { "path": "@.0", "label": "To", "format": "address" },
                            { "path": "@.1", "label": "Amount", "format": "number" }
                        ]
                    }
                }
            }
        }
        """#

        let calldataHex = "a9059cbb000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000003e8"
        let to = "0xdac17f958d2ee523a2206206994597c13d831ec7"

        do {
            let model = try erc7730FormatCalldata(
                descriptorJson: descriptorJson,
                chainId: 1,
                to: to,
                calldataHex: calldataHex,
                valueHex: nil,
                tokens: []
            )
            status = "OK: \(model.intent) | entries=\(model.entries.count)"
        } catch {
            status = "Error: \(error.localizedDescription)"
        }
    }
}
