package com.clearsigning

import uniffi.clear_signing.TransactionInput
import uniffi.clear_signing.clearSigningFormatCalldata
import uniffi.clear_signing.clearSigningFormatTypedData
import uniffi.clear_signing.clearSigningResolveDescriptorsForTx
import uniffi.clear_signing.clearSigningResolveDescriptorsForTypedData

class ClearSigningClient(
    private val dataProvider: DataProviderFfi,
) {
    suspend fun formatCalldata(
        chainId: ULong,
        to: String,
        calldataHex: String,
        valueHex: String? = null,
        fromAddress: String? = null,
    ): DisplayModel {
        val transaction = TransactionInput(
            chainId = chainId,
            to = to,
            calldataHex = calldataHex,
            valueHex = valueHex,
            fromAddress = fromAddress,
        )
        val descriptors = resolveDescriptorsForTx(transaction)
        return clearSigningFormatCalldata(
            descriptorsJson = descriptors,
            transaction = transaction,
            dataProvider = dataProvider,
        )
    }

    suspend fun formatTypedData(
        typedDataJson: String,
    ): DisplayModel {
        val descriptors = resolveDescriptorsForTypedData(typedDataJson)
        return clearSigningFormatTypedData(
            descriptorsJson = descriptors,
            typedDataJson = typedDataJson,
            dataProvider = dataProvider,
        )
    }

    suspend fun resolveDescriptorsForTx(
        chainId: ULong,
        to: String,
        calldataHex: String,
        valueHex: String? = null,
        fromAddress: String? = null,
    ): List<String> {
        val transaction = TransactionInput(
            chainId = chainId,
            to = to,
            calldataHex = calldataHex,
            valueHex = valueHex,
            fromAddress = fromAddress,
        )
        return resolveDescriptorsForTx(transaction)
    }

    suspend fun resolveDescriptorsForTypedData(
        typedDataJson: String,
    ): List<String> =
        clearSigningResolveDescriptorsForTypedData(
            typedDataJson = typedDataJson,
            dataProvider = dataProvider,
        )

    private suspend fun resolveDescriptorsForTx(
        transaction: TransactionInput,
    ): List<String> =
        clearSigningResolveDescriptorsForTx(
            transaction = transaction,
            dataProvider = dataProvider,
        )
}
