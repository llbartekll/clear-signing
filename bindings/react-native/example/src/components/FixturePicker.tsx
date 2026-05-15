import React from 'react';
import {
  ScrollView,
  StyleSheet,
  Text,
  TouchableOpacity,
} from 'react-native';
import type {Fixture} from '../fixtures';

type Props = {
  fixtures: Fixture[];
  selectedId: string;
  onSelect: (id: string) => void;
};

export function FixturePicker({fixtures, selectedId, onSelect}: Props) {
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      contentContainerStyle={styles.row}>
      {fixtures.map(f => {
        const selected = f.id === selectedId;
        return (
          <TouchableOpacity
            key={f.id}
            style={[styles.pill, selected && styles.pillSelected]}
            onPress={() => onSelect(f.id)}>
            <Text style={[styles.label, selected && styles.labelSelected]}>
              {f.label}
            </Text>
            <Text style={[styles.subLabel, selected && styles.subLabelSelected]}>
              {f.contractName}
            </Text>
          </TouchableOpacity>
        );
      })}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  row: {
    paddingHorizontal: 12,
    paddingVertical: 8,
    gap: 8,
  },
  pill: {
    backgroundColor: '#fff',
    borderRadius: 12,
    paddingVertical: 10,
    paddingHorizontal: 14,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: '#cbd5e1',
    minWidth: 140,
  },
  pillSelected: {
    backgroundColor: '#2563eb',
    borderColor: '#2563eb',
  },
  label: {fontSize: 14, fontWeight: '600', color: '#111'},
  labelSelected: {color: '#fff'},
  subLabel: {fontSize: 12, color: '#64748b', marginTop: 2},
  subLabelSelected: {color: '#dbeafe'},
});
