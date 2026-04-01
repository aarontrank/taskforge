import fs from 'fs/promises';
import { Config, ConfigSchema } from '../models/types.js';
import { configPath } from './paths.js';

export class ConfigStorage {
  constructor(private root: string) {}

  async read(): Promise<Config> {
    try {
      const content = await fs.readFile(configPath(this.root), 'utf-8');
      return ConfigSchema.parse(JSON.parse(content));
    } catch {
      return ConfigSchema.parse({});
    }
  }

  async write(config: Config): Promise<void> {
    await fs.writeFile(configPath(this.root), JSON.stringify(config, null, 2), 'utf-8');
  }
}
