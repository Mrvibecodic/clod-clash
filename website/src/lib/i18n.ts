import { defineI18n } from 'fumadocs-core/i18n'
import { uiTranslations } from 'fumadocs-ui/i18n'

export const i18n = defineI18n({
  languages: ['ru', 'en'],
  defaultLanguage: 'ru',
  hideLocale: 'never',
  parser: 'dir',
})

export const translations = i18n
  .translations()
  .extend(uiTranslations())
  .add({
    en: {
      displayName: 'English',
    },
    ru: {
      displayName: 'Русский',
      'Ask AI(AI chat button)': 'Спросить ИИ',
      'Back to Home(404 not found page)': 'На главную',
      'Choose a language(language switcher)': 'Выберите язык',
      'Choose a language(language switcher)(aria-label)': 'Выберите язык',
      'Close Banner(banner)(aria-label)': 'Закрыть баннер',
      'Close Search(search dialog)(aria-label)': 'Закрыть поиск',
      'Close Sidebar(aria-label)': 'Закрыть боковую панель',
      'Close Sidebar(sidebar)(aria-label)': 'Закрыть боковую панель',
      'Collapse Sidebar(sidebar)(aria-label)': 'Свернуть боковую панель',
      'Copied Text(code block)(aria-label)': 'Скопировано',
      'Copy Anchor Link(heading anchor)(aria-label)': 'Скопировать ссылку на раздел',
      'Copy Link(accordion)(aria-label)': 'Скопировать ссылку',
      'Copy Text(code block)(aria-label)': 'Скопировать',
      'Dark(theme switcher)(aria-label)': 'Тёмная тема',
      'Default(type table)': 'По умолчанию',
      'Edit on GitHub(edit page)': 'Редактировать на GitHub',
      'Hide Sidebar(sidebar)': 'Скрыть боковую панель',
      'Last updated on(page footer)': 'Обновлено',
      'Layout Tab(layout tab trigger)': 'Раздел',
      'Light(theme switcher)(aria-label)': 'Светлая тема',
      'Next Page(pagination)': 'Следующая страница',
      'No Headings(table of contents)': 'Нет заголовков',
      'No results found(search dialog)': 'Ничего не найдено',
      'On this page(table of contents)': 'На этой странице',
      'Open Search(search trigger)(aria-label)': 'Открыть поиск',
      'Open Sidebar(aria-label)': 'Открыть боковую панель',
      'Open Sidebar(sidebar)(aria-label)': 'Открыть боковую панель',
      'Page Not Found(404 not found page)': 'Страница не найдена',
      'Parameters(type table)': 'Параметры',
      'Previous Page(pagination)': 'Предыдущая страница',
      'Prop(type table)': 'Свойство',
      'Returns(type table)': 'Возвращает',
      'Search(search dialog)': 'Поиск',
      'Search(search trigger)': 'Поиск',
      'Show Sidebar(sidebar)': 'Показать боковую панель',
      'System(theme switcher)(aria-label)': 'Системная тема',
      'Table of Contents(inline table of contents)': 'Содержание',
      'The page you are looking for might have been removed, had its name changed, or is temporarily unavailable.(404 not found page)':
        'Страница могла быть удалена, переименована или временно недоступна.',
      'Toggle Menu(home layout header)(aria-label)': 'Меню',
      'Toggle Theme(theme switcher)(aria-label)': 'Переключить тему',
      'Type(type table)': 'Тип',
    },
  })
