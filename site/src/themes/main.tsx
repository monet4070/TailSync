import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './showcase.css'
import { ThemesApp } from './ThemesApp.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemesApp />
  </StrictMode>,
)
