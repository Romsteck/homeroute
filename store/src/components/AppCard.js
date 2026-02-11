import { View, Text, TouchableOpacity, StyleSheet } from 'react-native';
import { Ionicons } from '@expo/vector-icons';

const formatSize = (bytes) =>
  bytes >= 1e6 ? (bytes / 1e6).toFixed(1) + ' MB' : (bytes / 1e3).toFixed(0) + ' KB';

export default function AppCard({ app, onPress }) {
  return (
    <TouchableOpacity style={styles.card} onPress={onPress} activeOpacity={0.7}>
      <View style={styles.row}>
        <View style={styles.iconBox}>
          <Ionicons name="cube-outline" size={24} color="#60a5fa" />
        </View>
        <View style={styles.info}>
          <Text style={styles.name} numberOfLines={1}>{app.name}</Text>
          <Text style={styles.category}>{app.category || 'other'}</Text>
        </View>
      </View>
      <View style={styles.footer}>
        <Text style={styles.meta}>
          {app.latest_version ? `v${app.latest_version}` : '—'}
          {app.latest_size_bytes ? ` · ${formatSize(app.latest_size_bytes)}` : ''}
        </Text>
        <Text style={styles.meta}>
          {app.release_count} release{app.release_count !== 1 ? 's' : ''}
        </Text>
      </View>
    </TouchableOpacity>
  );
}

const styles = StyleSheet.create({
  card: {
    backgroundColor: '#1f2937',
    borderRadius: 12,
    padding: 16,
    marginBottom: 12,
    borderWidth: 1,
    borderColor: '#374151',
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  iconBox: {
    width: 44,
    height: 44,
    borderRadius: 10,
    backgroundColor: '#1e3a5f',
    alignItems: 'center',
    justifyContent: 'center',
  },
  info: {
    flex: 1,
  },
  name: {
    fontSize: 16,
    fontWeight: '600',
    color: '#f9fafb',
  },
  category: {
    fontSize: 12,
    color: '#9ca3af',
    marginTop: 2,
  },
  footer: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    marginTop: 12,
  },
  meta: {
    fontSize: 12,
    color: '#6b7280',
  },
});
