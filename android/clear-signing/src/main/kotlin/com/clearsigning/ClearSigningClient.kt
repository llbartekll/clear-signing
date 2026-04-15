package com.clearsigning

import uniffi.clear_signing.TransactionInput
import uniffi.clear_signing.DescriptorResolutionOutcome
import uniffi.clear_signing.DisplayModel
import uniffi.clear_signing.FallbackReason
import uniffi.clear_signing.FormatDiagnostic
import uniffi.clear_signing.FormatFailure
import uniffi.clear_signing.FormatOutcome
import uniffi.clear_signing.clearSigningFormatCalldata
import uniffi.clear_signing.clearSigningFormatTypedData
import uniffi.clear_signing.clearSigningResolveDescriptorsForTx
import uniffi.clear_signing.clearSigningResolveDescriptorsForTypedData

val FormatOutcome.model: DisplayModel
    get() = when (this) {
        is FormatOutcome.ClearSigned -> model
        is FormatOutcome.Fallback -> model
    }

val FormatOutcome.diagnostics: List<FormatDiagnostic>
    get() = when (this) {
        is FormatOutcome.ClearSigned -> diagnostics
        is FormatOutcome.Fallback -> diagnostics
    }

val FormatOutcome.fallbackReason: FallbackReason?
    get() = when (this) {
        is FormatOutcome.ClearSigned -> null
        is FormatOutcome.Fallback -> reason
    }

val FormatOutcome.isClearSigned: Boolean
    get() = this is FormatOutcome.ClearSigned

val DescriptorResolutionOutcome.descriptors: List<String>
    get() = when (this) {
        is DescriptorResolutionOutcome.Found -> v1
        DescriptorResolutionOutcome.NotFound -> emptyList()
    }

val FormatFailure.failureMessage: String
    get() = when (this) {
        is FormatFailure.InvalidInput -> message
        is FormatFailure.InvalidDescriptor -> message
        is FormatFailure.ResolutionFailed -> message
        is FormatFailure.Internal -> message
    }

val FormatFailure.retryable: Boolean
    get() = when (this) {
        is FormatFailure.InvalidInput -> retryable
        is FormatFailure.InvalidDescriptor -> retryable
        is FormatFailure.ResolutionFailed -> retryable
        is FormatFailure.Internal -> retryable
    }

class ClearSigningClient(
    private val dataProvider: DataProviderFfi,
) {
    suspend fun formatCalldata(
        chainId: ULong,
        to: String,
        calldataHex: String,
        valueHex: String? = null,
        fromAddress: String? = null,
    ): FormatOutcome {
        val transaction = TransactionInput(
            chainId = chainId,
            to = to,
            calldataHex = calldataHex,
            valueHex = valueHex,
            fromAddress = fromAddress,
        )
        val descriptors = resolveDescriptorsForTx(transaction)
        return clearSigningFormatCalldata(
            descriptorsJson = descriptors.descriptors,
            transaction = transaction,
            dataProvider = dataProvider,
        )
    }

    suspend fun formatTypedData(
        typedDataJson: String,
    ): FormatOutcome {
        val descriptors = resolveDescriptorsForTypedData(typedDataJson)
        return clearSigningFormatTypedData(
            descriptorsJson = descriptors.descriptors,
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
    ): DescriptorResolutionOutcome {
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
    ): DescriptorResolutionOutcome =
        clearSigningResolveDescriptorsForTypedData(
            typedDataJson = typedDataJson,
            dataProvider = dataProvider,
        )

    private suspend fun resolveDescriptorsForTx(
        transaction: TransactionInput,
    ): DescriptorResolutionOutcome =
        clearSigningResolveDescriptorsForTx(
            transaction = transaction,
            dataProvider = dataProvider,
        )
}
