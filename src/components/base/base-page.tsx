import ArrowBackRounded from '@mui/icons-material/ArrowBackRounded'
import { IconButton, Typography } from '@mui/material'
import { useTheme } from '@mui/material/styles'
import React, { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate } from 'react-router'

import { BaseErrorBoundary } from './base-error-boundary'

interface Props {
  title?: React.ReactNode // the page title
  header?: React.ReactNode // something behind title
  contentStyle?: React.CSSProperties
  children?: ReactNode
  full?: boolean
}

export const BasePage: React.FC<Props> = (props) => {
  const { title, header, contentStyle, full, children } = props
  const { t } = useTranslation()
  const theme = useTheme()
  const navigate = useNavigate()
  const location = useLocation()

  // clod:design-v2 — with no sidebar the inner pages need a way home.
  const showBack = location.pathname !== '/'

  return (
    <BaseErrorBoundary
      atHome={!showBack}
      onNavigate={(path) => void navigate(path)}
    >
      <div className="base-page">
        <header data-tauri-drag-region="true" style={{ userSelect: 'none' }}>
          <Typography
            sx={{
              fontSize: '20px',
              fontWeight: '700 ',
              display: 'flex',
              alignItems: 'center',
              gap: 1,
            }}
            data-tauri-drag-region="true"
          >
            {showBack ? (
              <IconButton
                size="small"
                aria-label={t('shared.actions.previous')}
                title={t('shared.actions.previous')}
                onClick={() => void navigate('/')}
                sx={{ mr: 0.25 }}
              >
                <ArrowBackRounded fontSize="small" />
              </IconButton>
            ) : null}
            <span
              data-tauri-drag-region="true"
              style={{
                minWidth: 0,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {title}
            </span>
          </Typography>

          {header}
        </header>

        <div
          className={full ? 'base-container no-padding' : 'base-container'}
          style={{ backgroundColor: theme.palette.background.default }}
        >
          <section
            style={{
              backgroundColor: theme.palette.background.default,
            }}
          >
            <div className="base-content" style={contentStyle}>
              {children}
            </div>
          </section>
        </div>
      </div>
    </BaseErrorBoundary>
  )
}
