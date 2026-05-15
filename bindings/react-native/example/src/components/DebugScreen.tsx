import React, {useEffect, useMemo, useState} from 'react';
import {SafeAreaView, ScrollView, StatusBar, StyleSheet, Text, View} from 'react-native';
import {
  DescriptorResolutionOutcome_Tags,
  clearSigningFormatCalldata,
  clearSigningResolveDescriptorsForTx,
  type TransactionInput,
} from 'react-native-clear-signing';
import {DemoDataProvider} from '../DemoDataProvider';
import {FIXTURES, type Fixture} from '../fixtures';
import {ClearTransactionCard, type Result} from './ClearTransactionCard';
import {FixturePicker} from './FixturePicker';
import {RawTransactionCard} from './RawTransactionCard';

export function DebugScreen() {
  const [selectedId, setSelectedId] = useState(FIXTURES[0].id);
  const [result, setResult] = useState<Result>({kind: 'idle'});

  const fixture = useMemo<Fixture>(
    () => FIXTURES.find(f => f.id === selectedId) ?? FIXTURES[0],
    [selectedId],
  );

  useEffect(() => {
    let cancelled = false;
    setResult({kind: 'loading'});

    const tx: TransactionInput = {
      chainId: fixture.chainId,
      to: fixture.to,
      calldataHex: fixture.calldataHex,
      valueHex: fixture.valueHex,
      fromAddress: undefined,
    };
    const provider = new DemoDataProvider();

    (async () => {
      try {
        const resolution = await clearSigningResolveDescriptorsForTx(
          tx,
          provider,
        );
        if (cancelled) return;
        if (resolution.tag === DescriptorResolutionOutcome_Tags.NotFound) {
          setResult({kind: 'notFound'});
          return;
        }
        const descriptors = resolution.inner[0];
        const outcome = await clearSigningFormatCalldata(
          descriptors,
          tx,
          provider,
        );
        if (cancelled) return;
        setResult({kind: 'outcome', outcome});
      } catch (e: any) {
        if (cancelled) return;
        const inner = e?.inner;
        const message = inner?.message ?? e?.message ?? String(e);
        setResult({kind: 'error', message, retryable: inner?.retryable});
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [fixture]);

  return (
    <SafeAreaView style={styles.root}>
      <StatusBar barStyle="dark-content" />
      <Text style={styles.title}>clear-signing demo</Text>
      <FixturePicker
        fixtures={FIXTURES}
        selectedId={selectedId}
        onSelect={setSelectedId}
      />
      <ScrollView contentContainerStyle={styles.scroll}>
        <RawTransactionCard fixture={fixture} />
        <ClearTransactionCard result={result} />
        <View style={styles.spacer} />
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: {flex: 1, backgroundColor: '#f8fafc'},
  title: {
    fontSize: 20,
    fontWeight: '700',
    color: '#111',
    paddingHorizontal: 16,
    paddingTop: 12,
  },
  scroll: {
    paddingHorizontal: 12,
    paddingBottom: 32,
  },
  spacer: {height: 24},
});
