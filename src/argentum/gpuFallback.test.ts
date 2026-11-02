import { describe, expect, it } from 'vitest';
import '../i18n';
import { registerArgentumTranslations } from './locales';
import { gpuFallbackMessage } from './gpuFallback';

registerArgentumTranslations();

describe('the GPU fallback message', () => {
  it('says the setting stays when OpenGL is for this launch only', () => {
    const message = gpuFallbackMessage({ kind: 'thisLaunch', backend: 'vulkan' });
    expect(message).toContain('using OpenGL for this session');
    expect(message).toContain('stays on Vulkan');
  });

  it('says the setting changed when OpenGL was saved', () => {
    const message = gpuFallbackMessage({ kind: 'saved', backend: 'dx12' });
    expect(message).toContain('DirectX 12');
    expect(message).toContain('changed the Processing Backend setting to OpenGL');
  });

  it('names a default setting the way Settings does', () => {
    expect(gpuFallbackMessage({ kind: 'thisLaunch', backend: 'auto' })).toContain('stays on Auto');
  });

  it('shows a backend it has no name for as it is', () => {
    expect(gpuFallbackMessage({ kind: 'thisLaunch', backend: 'webgpu' })).toContain('stays on webgpu');
  });
});
