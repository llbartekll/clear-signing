import React from 'react';
import {ActivityIndicator, StyleSheet, Text, View} from 'react-native';
import {
  type DisplayModel,
  type FormatOutcome,
  FormatOutcome_Tags,
} from 'react-native-clear-signing';
import {EntryRow} from './EntryRow';

export type Result =
  | {kind: 'idle'}
  | {kind: 'loading'}
  | {kind: 'notFound'}
  | {kind: 'outcome'; outcome: FormatOutcome}
  | {kind: 'error'; message: string; retryable?: boolean};

export function ClearTransactionCard({result}: {result: Result}) {
  return (
    <View style={styles.card}>
      <Text style={styles.title}>Clear Transaction</Text>
      <Body result={result} />
    </View>
  );
}

function Body({result}: {result: Result}) {
  if (result.kind === 'idle') {
    return <Text style={styles.muted}>Pick a transaction to format.</Text>;
  }
  if (result.kind === 'loading') {
    return (
      <View style={styles.loadingRow}>
        <ActivityIndicator />
        <Text style={styles.muted}>Resolving descriptor…</Text>
      </View>
    );
  }
  if (result.kind === 'notFound') {
    return (
      <View style={[styles.banner, styles.bannerYellow]}>
        <Text style={styles.bannerYellowText}>
          No descriptor in the registry for this contract.
        </Text>
      </View>
    );
  }
  if (result.kind === 'error') {
    return (
      <View style={[styles.banner, styles.bannerRed]}>
        <Text style={styles.bannerRedText}>{result.message}</Text>
        {result.retryable && (
          <Text style={styles.bannerRedHint}>Retryable.</Text>
        )}
      </View>
    );
  }

  const {outcome} = result;
  if (outcome.tag === FormatOutcome_Tags.ClearSigned) {
    const {model} = outcome.inner;
    return (
      <>
        <Badge variant="green" text="Clear Signed" />
        <ModelBody model={model} />
      </>
    );
  }
  // Fallback
  const {model, reason} = outcome.inner;
  return (
    <>
      <Badge variant="yellow" text={`Fallback: ${reason}`} />
      <ModelBody model={model} />
    </>
  );
}

function ModelBody({model}: {model: DisplayModel}) {
  return (
    <View>
      <Text style={styles.intent}>{model.intent}</Text>
      {model.contractName ? (
        <Text style={styles.subhead}>Interacting with: {model.contractName}</Text>
      ) : null}
      <View style={styles.entries}>
        {model.entries.map((e, i) => (
          <EntryRow key={i} entry={e} />
        ))}
      </View>
    </View>
  );
}

function Badge({
  variant,
  text,
}: {
  variant: 'green' | 'yellow';
  text: string;
}) {
  const v = variant === 'green' ? styles.badgeGreen : styles.badgeYellow;
  const t =
    variant === 'green' ? styles.badgeGreenText : styles.badgeYellowText;
  return (
    <View style={[styles.badge, v]}>
      <Text style={t}>✓ {text}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    backgroundColor: '#fff',
    borderRadius: 12,
    padding: 16,
    marginBottom: 12,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: '#e2e8f0',
  },
  title: {
    fontSize: 18,
    fontWeight: '700',
    color: '#111',
    marginBottom: 12,
  },
  muted: {color: '#64748b', fontSize: 14},
  loadingRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingVertical: 8,
  },
  badge: {
    paddingVertical: 8,
    paddingHorizontal: 12,
    borderRadius: 8,
    marginBottom: 12,
  },
  badgeGreen: {backgroundColor: '#dcfce7'},
  badgeYellow: {backgroundColor: '#fef3c7'},
  badgeGreenText: {color: '#15803d', fontWeight: '600'},
  badgeYellowText: {color: '#a16207', fontWeight: '600'},
  banner: {paddingVertical: 10, paddingHorizontal: 12, borderRadius: 8},
  bannerYellow: {backgroundColor: '#fef3c7'},
  bannerYellowText: {color: '#92400e', fontSize: 14},
  bannerRed: {backgroundColor: '#fee2e2'},
  bannerRedText: {color: '#b91c1c', fontSize: 14},
  bannerRedHint: {color: '#b91c1c', fontSize: 12, marginTop: 4, opacity: 0.7},
  intent: {fontSize: 18, fontWeight: '600', color: '#111', marginBottom: 4},
  subhead: {fontSize: 13, color: '#64748b', marginBottom: 8},
  entries: {marginTop: 8},
});
