import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './landing.css'
import App from './App.tsx'
import { applyMotionVariantEarly } from './hooks/useMotionVariant'

// Before the first paint — see the comment on applyMotionVariantEarly. No-op in
// production builds, where no variant attribute is written at all.
applyMotionVariantEarly()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
