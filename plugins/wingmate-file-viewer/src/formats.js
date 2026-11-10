// Only advertise formats whose renderers and offline dependencies ship with this plugin.
export const formats = [
  { id: 'word', extensions: ['doc', 'docx', 'odt'], binary: ['doc', 'docx', 'odt'] },
  { id: 'presentation', extensions: ['ppt', 'pptx'], binary: ['ppt', 'pptx'] },
  { id: 'ebook', extensions: ['epub', 'fb2', 'umd'], binary: ['epub', 'umd'] },
  { id: 'email', extensions: ['eml', 'msg'], binary: ['msg'] },
  { id: 'media', extensions: ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'mp4', 'webm'],
    binary: ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'mp4', 'webm'] },
];
