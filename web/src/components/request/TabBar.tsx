import { useStore } from '../../lib/store';
import { Plus, X } from 'lucide-react';

export function TabBar() {
  const tabs = useStore(s => s.tabs);
  const activeTabId = useStore(s => s.activeTabId);
  const setActiveTab = useStore(s => s.setActiveTab);
  const removeTab = useStore(s => s.removeTab);
  const addTab = useStore(s => s.addTab);

  if (!tabs || tabs.length === 0) return null;

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 0, marginBottom: 8,
      borderBottom: '1px solid var(--border)', minHeight: 32, overflow: 'hidden',
    }}>
      {}
      <div style={{
        display: 'flex', alignItems: 'stretch', gap: 0, flex: 1, overflowX: 'auto',
        scrollbarWidth: 'thin',
      }}>
        {tabs.map(tab => {
          const isActive = tab.id === activeTabId;
          return (
            <div key={tab.id} style={{
              display: 'flex', alignItems: 'center', gap: 4,
              padding: '5px 8px 5px 12px', fontSize: 12, cursor: 'pointer',
              whiteSpace: 'nowrap', flexShrink: 0,
              borderBottom: isActive ? '2px solid var(--accent)' : '2px solid transparent',
              color: isActive ? 'var(--accent)' : 'var(--text-secondary)',
              fontWeight: isActive ? 600 : 400,
              background: isActive ? 'var(--bg-primary)' : 'transparent',
              transition: 'all 0.1s ease',
              minWidth: 80, maxWidth: 180,
            }}
              onClick={() => setActiveTab(tab.id)}
              onMouseEnter={e => { if (!isActive) e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
              onMouseLeave={e => { if (!isActive) e.currentTarget.style.background = 'transparent'; }}
            >
              <span style={{
                overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1,
              }}>
                {tab.label}
              </span>
              <button onClick={e => { e.stopPropagation(); removeTab(tab.id); }} style={{
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                width: 18, height: 18, borderRadius: 3, border: 'none', background: 'none',
                cursor: 'pointer', color: isActive ? 'var(--accent)' : 'var(--text-muted)',
                opacity: 0, flexShrink: 0,
                transition: 'all 0.1s ease',
              }}
                className="tab-close-btn"
                onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; e.currentTarget.style.opacity = '1'; }}
                onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.opacity = '0'; }}
              >
                <X size={12} />
              </button>
            </div>
          );
        })}

        {}
        <style>{`
          .tab-close-btn { opacity: 0; }
          div:has(> .tab-close-btn):hover .tab-close-btn { opacity: 1 !important; }
        `}</style>
      </div>

      {}
      <button onClick={() => addTab()} style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        width: 28, height: 28, borderRadius: 4, border: 'none', background: 'none',
        cursor: 'pointer', color: 'var(--text-muted)', flexShrink: 0,
        transition: 'all 0.1s ease', marginRight: 2,
      }}
        onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; e.currentTarget.style.color = 'var(--text-primary)'; }}
        onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-muted)'; }}
        title="New tab"
      >
        <Plus size={16} />
      </button>
    </div>
  );
}
