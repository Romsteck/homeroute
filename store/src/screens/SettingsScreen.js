import { useState, useEffect } from 'react';
import {
  View, Text, TextInput, TouchableOpacity,
  StyleSheet, Alert, ActivityIndicator,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import { getServerUrl, setServerUrl, getStoreApps } from '../api/client';

export default function SettingsScreen({ navigation }) {
  const [url, setUrl] = useState('');
  const [testing, setTesting] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    (async () => {
      const stored = await getServerUrl();
      if (stored) setUrl(stored);
    })();
  }, []);

  const handleTest = async () => {
    if (!url.trim()) return;
    setTesting(true);
    setSaved(false);
    try {
      await setServerUrl(url.trim());
      await getStoreApps();
      setSaved(true);
      setTimeout(() => navigation.navigate('Catalog'), 500);
    } catch (err) {
      Alert.alert('Connexion echouee', err.message);
    } finally {
      setTesting(false);
    }
  };

  return (
    <View style={styles.container}>
      <View style={styles.card}>
        <View style={styles.iconRow}>
          <Ionicons name="server-outline" size={32} color="#60a5fa" />
        </View>
        <Text style={styles.title}>Serveur HomeRoute</Text>
        <Text style={styles.subtitle}>
          Entrez l'URL de votre serveur HomeRoute pour acceder au store.
        </Text>

        <Text style={styles.label}>URL du serveur</Text>
        <TextInput
          style={styles.input}
          value={url}
          onChangeText={setUrl}
          placeholder="https://homeroute.local:4000"
          placeholderTextColor="#4b5563"
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
        />

        <TouchableOpacity
          style={[styles.btn, (!url.trim() || testing) && styles.btnDisabled]}
          onPress={handleTest}
          disabled={!url.trim() || testing}
          activeOpacity={0.7}
        >
          {testing ? (
            <ActivityIndicator size="small" color="#fff" />
          ) : saved ? (
            <Ionicons name="checkmark-circle" size={20} color="#fff" />
          ) : (
            <Ionicons name="link-outline" size={20} color="#fff" />
          )}
          <Text style={styles.btnText}>
            {testing ? 'Test en cours...' : saved ? 'Connecte' : 'Connecter'}
          </Text>
        </TouchableOpacity>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#111827',
    justifyContent: 'center',
    padding: 20,
  },
  card: {
    backgroundColor: '#1f2937',
    borderRadius: 16,
    padding: 24,
    borderWidth: 1,
    borderColor: '#374151',
  },
  iconRow: {
    alignItems: 'center',
    marginBottom: 16,
  },
  title: {
    fontSize: 20,
    fontWeight: '700',
    color: '#f9fafb',
    textAlign: 'center',
  },
  subtitle: {
    fontSize: 13,
    color: '#9ca3af',
    textAlign: 'center',
    marginTop: 6,
    marginBottom: 24,
    lineHeight: 18,
  },
  label: {
    fontSize: 13,
    color: '#9ca3af',
    marginBottom: 6,
  },
  input: {
    backgroundColor: '#111827',
    borderWidth: 1,
    borderColor: '#4b5563',
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 12,
    fontSize: 15,
    color: '#f9fafb',
    marginBottom: 20,
  },
  btn: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    backgroundColor: '#2563eb',
    borderRadius: 10,
    paddingVertical: 14,
  },
  btnDisabled: {
    opacity: 0.5,
  },
  btnText: {
    color: '#fff',
    fontSize: 15,
    fontWeight: '600',
  },
});
