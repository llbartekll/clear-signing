import React, {useState} from 'react';
import {StyleSheet, Text, TouchableOpacity, View} from 'react-native';
import type {Fixture} from '../fixtures';

type Props = {
  fixture: Fixture;
};

const SHORT_CALLDATA_CHARS = 80;

function formatValue(valueHex?: string): string {
  if (!valueHex || valueHex === '0x0' || valueHex === '0x') return '0 ETH';
  try {
    const wei = BigInt(valueHex);
    if (wei === 0n) return '0 ETH';
    const whole = wei / 10n ** 18n;
    const frac = wei % 10n ** 18n;
    const fracStr = frac.toString().padStart(18, '0').replace(/0+$/, '');
    return fracStr ? `${whole}.${fracStr} ETH` : `${whole} ETH`;
  } catch {
    return valueHex;
  }
}

export function RawTransactionCard({fixture}: Props) {
  const [showFull, setShowFull] = useState(false);
  const calldata = fixture.calldataHex;
  const truncated =
    calldata.length > SHORT_CALLDATA_CHARS
      ? `${calldata.slice(0, SHORT_CALLDATA_CHARS)}…`
      : calldata;

  return (
    <View style={styles.card}>
      <Text style={styles.title}>Debug</Text>
      <Row label="Chain ID" value={fixture.chainId.toString()} />
      <Row label="To" value={fixture.to} mono />
      <Row label="Value" value={formatValue(fixture.valueHex)} />
      <View style={styles.row}>
        <Text style={styles.label}>Calldata</Text>
        <View style={styles.valueBox}>
          <Text style={[styles.value, styles.mono]} selectable>
            {showFull ? calldata : truncated}
          </Text>
          {calldata.length > SHORT_CALLDATA_CHARS && (
            <TouchableOpacity onPress={() => setShowFull(!showFull)}>
              <Text style={styles.toggle}>
                {showFull ? 'Show less' : 'Show full'}
              </Text>
            </TouchableOpacity>
          )}
        </View>
      </View>
    </View>
  );
}

function Row({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <View style={styles.row}>
      <Text style={styles.label}>{label}</Text>
      <Text style={[styles.value, mono && styles.mono]} selectable>
        {value}
      </Text>
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
    marginBottom: 12,
    color: '#111',
    textAlign: 'center',
  },
  row: {
    flexDirection: 'row',
    paddingVertical: 8,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: '#f1f5f9',
    gap: 12,
  },
  label: {
    flex: 1,
    color: '#94a3b8',
    fontSize: 14,
  },
  valueBox: {
    flex: 2,
  },
  value: {
    flex: 2,
    color: '#111',
    fontSize: 14,
  },
  mono: {
    fontFamily: 'Menlo',
    fontSize: 12,
  },
  toggle: {
    color: '#2563eb',
    fontSize: 14,
    marginTop: 6,
  },
});
