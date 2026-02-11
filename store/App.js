import { NavigationContainer, DarkTheme } from '@react-navigation/native';
import { createNativeStackNavigator } from '@react-navigation/native-stack';
import { StatusBar } from 'expo-status-bar';
import { TouchableOpacity } from 'react-native';
import { Ionicons } from '@expo/vector-icons';

import CatalogScreen from './src/screens/CatalogScreen';
import AppDetailScreen from './src/screens/AppDetailScreen';
import SettingsScreen from './src/screens/SettingsScreen';

const Stack = createNativeStackNavigator();

const navTheme = {
  ...DarkTheme,
  colors: {
    ...DarkTheme.colors,
    background: '#111827',
    card: '#1f2937',
    border: '#374151',
    primary: '#60a5fa',
    text: '#f9fafb',
  },
};

export default function App() {
  return (
    <>
      <StatusBar style="light" />
      <NavigationContainer theme={navTheme}>
        <Stack.Navigator
          screenOptions={{
            headerStyle: { backgroundColor: '#1f2937' },
            headerTintColor: '#f9fafb',
            headerTitleStyle: { fontWeight: '600' },
          }}
        >
          <Stack.Screen
            name="Catalog"
            component={CatalogScreen}
            options={({ navigation }) => ({
              title: 'Store',
              headerRight: () => (
                <TouchableOpacity onPress={() => navigation.navigate('Settings')}>
                  <Ionicons name="settings-outline" size={22} color="#9ca3af" />
                </TouchableOpacity>
              ),
            })}
          />
          <Stack.Screen
            name="AppDetail"
            component={AppDetailScreen}
            options={({ route }) => ({
              title: route.params?.name || 'Details',
            })}
          />
          <Stack.Screen
            name="Settings"
            component={SettingsScreen}
            options={{ title: 'Configuration' }}
          />
        </Stack.Navigator>
      </NavigationContainer>
    </>
  );
}
