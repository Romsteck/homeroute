import { useState, useCallback } from 'react';
import {
  View, Text, FlatList, ActivityIndicator,
  StyleSheet, RefreshControl,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import { Ionicons } from '@expo/vector-icons';
import { getStoreApps, getServerUrl } from '../api/client';
import AppCard from '../components/AppCard';

export default function CatalogScreen({ navigation }) {
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState(null);

  const fetchApps = useCallback(async (isRefresh = false) => {
    if (isRefresh) setRefreshing(true);
    setError(null);
    try {
      const serverUrl = await getServerUrl();
      if (!serverUrl) {
        navigation.replace('Settings');
        return;
      }
      const data = await getStoreApps();
      setApps(data.apps || []);
    } catch (err) {
      setError(err.message);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [navigation]);

  useFocusEffect(
    useCallback(() => {
      setLoading(true);
      fetchApps();
    }, [fetchApps])
  );

  const totalReleases = apps.reduce((sum, a) => sum + (a.release_count || 0), 0);

  if (loading && !refreshing) {
    return (
      <View style={styles.center}>
        <ActivityIndicator size="large" color="#60a5fa" />
      </View>
    );
  }

  return (
    <View style={styles.container}>
      {error && (
        <View style={styles.errorBox}>
          <Text style={styles.errorText}>{error}</Text>
        </View>
      )}

      <View style={styles.statsRow}>
        <View style={styles.stat}>
          <Ionicons name="cube-outline" size={16} color="#60a5fa" />
          <Text style={styles.statText}>{apps.length} app{apps.length !== 1 ? 's' : ''}</Text>
        </View>
        <View style={styles.stat}>
          <Ionicons name="pricetag-outline" size={16} color="#34d399" />
          <Text style={styles.statText}>{totalReleases} release{totalReleases !== 1 ? 's' : ''}</Text>
        </View>
      </View>

      <FlatList
        data={apps}
        keyExtractor={(item) => item.slug}
        contentContainerStyle={styles.list}
        renderItem={({ item }) => (
          <AppCard
            app={item}
            onPress={() => navigation.navigate('AppDetail', { slug: item.slug, name: item.name })}
          />
        )}
        ListEmptyComponent={
          <View style={styles.empty}>
            <Ionicons name="storefront-outline" size={48} color="#4b5563" />
            <Text style={styles.emptyText}>Aucune application</Text>
            <Text style={styles.emptySubtext}>Les publications sont gerees via MCP.</Text>
          </View>
        }
        refreshControl={
          <RefreshControl
            refreshing={refreshing}
            onRefresh={() => fetchApps(true)}
            tintColor="#60a5fa"
            colors={['#60a5fa']}
          />
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#111827',
  },
  center: {
    flex: 1,
    backgroundColor: '#111827',
    alignItems: 'center',
    justifyContent: 'center',
  },
  errorBox: {
    marginHorizontal: 16,
    marginTop: 12,
    padding: 12,
    borderRadius: 8,
    backgroundColor: '#450a0a',
    borderWidth: 1,
    borderColor: '#7f1d1d',
  },
  errorText: {
    color: '#fca5a5',
    fontSize: 13,
  },
  statsRow: {
    flexDirection: 'row',
    gap: 20,
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderBottomWidth: 1,
    borderBottomColor: '#1f2937',
  },
  stat: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  statText: {
    color: '#9ca3af',
    fontSize: 13,
  },
  list: {
    padding: 16,
  },
  empty: {
    alignItems: 'center',
    paddingTop: 80,
  },
  emptyText: {
    color: '#9ca3af',
    fontSize: 16,
    marginTop: 12,
  },
  emptySubtext: {
    color: '#6b7280',
    fontSize: 13,
    marginTop: 4,
  },
});
