import React from 'react';
import {StyleSheet, Text, View} from 'react-native';
import {
  type DisplayEntry,
  type DisplayItem,
  DisplayEntry_Tags,
} from 'react-native-clear-signing';

type Props = {
  entry: DisplayEntry;
  depth?: number;
};

export function EntryRow({entry, depth = 0}: Props) {
  const indent = depth * 12;

  if (entry.tag === DisplayEntry_Tags.Item) {
    const item = entry.inner[0];
    return <ItemLine item={item} indent={indent} />;
  }

  if (entry.tag === DisplayEntry_Tags.Group) {
    const {label, items} = entry.inner;
    return (
      <View style={[styles.groupBlock, {marginLeft: indent}]}>
        <Text style={styles.groupLabel}>{label}</Text>
        {items.map((item, i) => (
          <ItemLine key={`${label}:${i}`} item={item} indent={0} />
        ))}
      </View>
    );
  }

  const {label, intent, entries} = entry.inner;
  return (
    <View style={[styles.nestedBlock, {marginLeft: indent}]}>
      <Text style={styles.nestedLabel}>{label}</Text>
      <Text style={styles.nestedIntent}>{intent}</Text>
      {entries.map((child, i) => (
        <EntryRow key={`${label}:${i}`} entry={child} depth={depth + 1} />
      ))}
    </View>
  );
}

function ItemLine({item, indent}: {item: DisplayItem; indent: number}) {
  return (
    <View style={[styles.itemRow, {marginLeft: indent}]}>
      <Text style={styles.itemLabel}>{item.label}</Text>
      <Text style={styles.itemValue} selectable>
        {item.value}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  itemRow: {
    flexDirection: 'row',
    paddingVertical: 6,
    gap: 12,
  },
  itemLabel: {
    flex: 1,
    color: '#666',
    fontSize: 14,
    textAlign: 'right',
  },
  itemValue: {
    flex: 2,
    color: '#111',
    fontSize: 14,
    fontVariant: ['tabular-nums'],
  },
  groupBlock: {
    marginVertical: 4,
  },
  groupLabel: {
    color: '#444',
    fontSize: 13,
    fontWeight: '600',
    marginBottom: 2,
  },
  nestedBlock: {
    marginVertical: 6,
    paddingLeft: 8,
    borderLeftWidth: 2,
    borderLeftColor: '#cbd5e1',
  },
  nestedLabel: {
    color: '#444',
    fontSize: 13,
    fontWeight: '600',
  },
  nestedIntent: {
    color: '#111',
    fontSize: 14,
    marginBottom: 4,
  },
});
