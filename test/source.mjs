import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const videoRoot = resolve(process.env.RELAY_VIDEO_SOURCE || resolve(import.meta.dirname, '../../..', 'chrome-extensions/story-studio-video-generator'));
export const videoModule = (name) => import(pathToFileURL(resolve(videoRoot, 'src/lib', name)).href);
