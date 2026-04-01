package com.clearsigning.smoke

import com.clearsigning.ClearSigningClient
import com.clearsigning.DataProviderFfi
import com.clearsigning.TokenMetaFfi

private class FakeDataProvider : DataProviderFfi {
    override fun resolveToken(chainId: ULong, address: String): TokenMetaFfi? = null

    override fun resolveEnsName(address: String, chainId: ULong, types: List<String>?): String? = null

    override fun resolveLocalName(address: String, chainId: ULong, types: List<String>?): String? = null

    override fun resolveNftCollectionName(collectionAddress: String, chainId: ULong): String? = null

    override fun resolveBlockTimestamp(chainId: ULong, blockNumber: ULong): ULong? = null

    override fun getImplementationAddress(chainId: ULong, address: String): String? = null
}

object Smoke {
    val client = ClearSigningClient(FakeDataProvider())

    suspend fun compileOnlyReference() {
        client.formatCalldata(
            chainId = 1uL,
            to = "0x0000000000000000000000000000000000000000",
            calldataHex = "0x"
        )
    }
}
