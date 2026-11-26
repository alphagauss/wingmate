import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from '../ui/App.tsx';
import { WebButton } from '../ui/controls.tsx';
import '../ui/styles.css';

createRoot(document.getElementById('root')!).render(<App Button={WebButton} />);
