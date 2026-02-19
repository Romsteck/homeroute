import { useState, useEffect, useCallback } from 'react';
import { getDataverseRelations } from '../../api/client';

export default function useLookupResolver(appId) {
  const [relations, setRelations] = useState([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!appId) { setRelations([]); return; }
    let cancelled = false;
    setLoading(true);
    getDataverseRelations(appId).then(res => {
      if (!cancelled) setRelations(res.data?.relations || []);
    }).catch(() => {
      if (!cancelled) setRelations([]);
    }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [appId]);

  const getRelation = useCallback((tableName, columnName) => {
    return relations.find(r => r.from_table === tableName && r.from_column === columnName);
  }, [relations]);

  const isLookup = useCallback((tableName, columnName) => {
    return !!getRelation(tableName, columnName);
  }, [getRelation]);

  return { relations, loading, getRelation, isLookup };
}
