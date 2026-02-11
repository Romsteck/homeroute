import { useState, useEffect } from 'react';
import {
  View, Text, ScrollView, TouchableOpacity,
  ActivityIndicator, StyleSheet, Alert, Platform
} from 'react-native';
import * as FileSystem from 'expo-file-system';
import { startActivityAsync } from 'expo-intent-launcher';
import { Ionicons } from '@expo/vector-icons';
import { getStoreApp, getDownloadUrl } from '../api/client';

const formatSize = (bytes) =>
  bytes >= 1e6 ? (bytes / 1e6).toFixed(1) + ' MB' : (bytes / 1e3).toFixed(0) + ' KB';

export default function AppDetailScreen({ route }) {
  const { slug } = route.params;
  const [app, setApp] = useState(null);
  const [loading, setLoading] = useState(true);
  const [downloading, setDownloading] = useState(null); // version string or null

  useEffect(() => {
    (async () => {
      try {
        const data = await getStoreApp(slug);
        setApp(data.app || null);
      } catch (err) {
        Alert.alert('Erreur', err.message);
      } finally {
        setLoading(false);
      }
    })();
  }, [slug]);

  const handleDownload = async (version) => {
    if (downloading) return;
    setDownloading(version);
    try {
      const url = getDownloadUrl(slug, version);
      const fileUri = FileSystem.cacheDirectory + `${slug}-${version}.apk`;
      const { uri } = await FileSystem.downloadAsync(url, fileUri);

      if (Platform.OS === 'android') {
        const contentUri = await FileSystem.getContentUriAsync(uri);
        await startActivityAsync('android.intent.action.VIEW', {
          data: contentUri,
          flags: 1, // FLAG_GRANT_READ_URI_PERMISSION
          type: 'application/vnd.android.package-archive',
        });
      } else {
        Alert.alert('Telecharge', `APK sauvegarde: ${uri}`);
      }
    } catch (err) {
      Alert.alert('Erreur de telechargement', err.message);
    } finally {
      setDownloading(null);
    }
  };

  if (loading) {
    return (
      <View style={styles.center}>
        <ActivityIndicator size="large" color="#60a5fa" />
      </View>
    );
  }

  if (!app) {
    return (
      <View style={styles.center}>
        <Ionicons name="alert-circle-outline" size={48} color="#6b7280" />
        <Text style={styles.emptyText}>Application introuvable</Text>
      </View>
    );
  }

  const releases = [...(app.releases || [])].reverse();

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      {/* App Info */}
      <View style={styles.section}>
        <View style={styles.headerRow}>
          <View style={styles.iconBox}>
            <Ionicons name="cube-outline" size={28} color="#60a5fa" />
          </View>
          <View style={styles.headerInfo}>
            <Text style={styles.appName}>{app.name}</Text>
            <Text style={styles.slug}>{app.slug}</Text>
          </View>
        </View>

        <View style={styles.metaGrid}>
          <View style={styles.metaItem}>
            <Text style={styles.metaLabel}>Categorie</Text>
            <Text style={styles.metaValue}>{app.category || 'other'}</Text>
          </View>
          <View style={styles.metaItem}>
            <Text style={styles.metaLabel}>Releases</Text>
            <Text style={styles.metaValue}>{releases.length}</Text>
          </View>
        </View>

        {app.description ? (
          <Text style={styles.description}>{app.description}</Text>
        ) : null}
      </View>

      {/* Download latest */}
      {releases.length > 0 && (
        <TouchableOpacity
          style={styles.downloadBtn}
          onPress={() => handleDownload(releases[0].version)}
          disabled={!!downloading}
          activeOpacity={0.7}
        >
          {downloading === releases[0].version ? (
            <ActivityIndicator size="small" color="#fff" />
          ) : (
            <Ionicons name="download-outline" size={20} color="#fff" />
          )}
          <Text style={styles.downloadBtnText}>
            Installer v{releases[0].version} ({formatSize(releases[0].size_bytes)})
          </Text>
        </TouchableOpacity>
      )}

      {/* Releases */}
      <Text style={styles.sectionTitle}>Releases</Text>
      {releases.map((rel) => (
        <View key={rel.version} style={styles.releaseCard}>
          <View style={styles.releaseHeader}>
            <View style={styles.releaseInfo}>
              <View style={styles.versionRow}>
                <Ionicons name="pricetag-outline" size={14} color="#60a5fa" />
                <Text style={styles.versionText}>v{rel.version}</Text>
              </View>
              <Text style={styles.releaseMeta}>
                {new Date(rel.created_at).toLocaleDateString('fr-FR')} · {formatSize(rel.size_bytes)}
              </Text>
            </View>
            <TouchableOpacity
              style={styles.dlSmallBtn}
              onPress={() => handleDownload(rel.version)}
              disabled={!!downloading}
            >
              {downloading === rel.version ? (
                <ActivityIndicator size="small" color="#60a5fa" />
              ) : (
                <Ionicons name="download-outline" size={18} color="#60a5fa" />
              )}
            </TouchableOpacity>
          </View>
          {rel.changelog ? (
            <Text style={styles.changelog}>{rel.changelog}</Text>
          ) : null}
          <Text style={styles.sha} numberOfLines={1}>SHA-256: {rel.sha256}</Text>
        </View>
      ))}

      {releases.length === 0 && (
        <Text style={styles.noReleases}>Aucune release.</Text>
      )}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#111827',
  },
  content: {
    padding: 16,
    paddingBottom: 40,
  },
  center: {
    flex: 1,
    backgroundColor: '#111827',
    alignItems: 'center',
    justifyContent: 'center',
  },
  emptyText: {
    color: '#9ca3af',
    marginTop: 12,
    fontSize: 15,
  },
  section: {
    backgroundColor: '#1f2937',
    borderRadius: 12,
    padding: 16,
    borderWidth: 1,
    borderColor: '#374151',
  },
  headerRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
    marginBottom: 16,
  },
  iconBox: {
    width: 52,
    height: 52,
    borderRadius: 12,
    backgroundColor: '#1e3a5f',
    alignItems: 'center',
    justifyContent: 'center',
  },
  headerInfo: {
    flex: 1,
  },
  appName: {
    fontSize: 20,
    fontWeight: '700',
    color: '#f9fafb',
  },
  slug: {
    fontSize: 13,
    color: '#6b7280',
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
    marginTop: 2,
  },
  metaGrid: {
    flexDirection: 'row',
    gap: 24,
  },
  metaItem: {},
  metaLabel: {
    fontSize: 11,
    color: '#6b7280',
    textTransform: 'uppercase',
    letterSpacing: 0.5,
  },
  metaValue: {
    fontSize: 14,
    color: '#e5e7eb',
    marginTop: 2,
  },
  description: {
    fontSize: 13,
    color: '#9ca3af',
    marginTop: 12,
    lineHeight: 18,
  },
  downloadBtn: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    backgroundColor: '#2563eb',
    borderRadius: 10,
    paddingVertical: 14,
    marginTop: 16,
  },
  downloadBtnText: {
    color: '#fff',
    fontSize: 15,
    fontWeight: '600',
  },
  sectionTitle: {
    fontSize: 13,
    fontWeight: '600',
    color: '#6b7280',
    textTransform: 'uppercase',
    letterSpacing: 0.5,
    marginTop: 24,
    marginBottom: 12,
  },
  releaseCard: {
    backgroundColor: '#1f2937',
    borderRadius: 10,
    padding: 14,
    marginBottom: 10,
    borderWidth: 1,
    borderColor: '#374151',
  },
  releaseHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  releaseInfo: {},
  versionRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  versionText: {
    fontSize: 15,
    fontWeight: '600',
    color: '#60a5fa',
  },
  releaseMeta: {
    fontSize: 12,
    color: '#6b7280',
    marginTop: 3,
  },
  dlSmallBtn: {
    padding: 8,
    borderRadius: 8,
    backgroundColor: '#1e3a5f',
  },
  changelog: {
    fontSize: 13,
    color: '#9ca3af',
    marginTop: 10,
    lineHeight: 18,
  },
  sha: {
    fontSize: 10,
    color: '#4b5563',
    marginTop: 8,
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
  },
  noReleases: {
    color: '#6b7280',
    fontSize: 14,
    textAlign: 'center',
    marginTop: 20,
  },
});
