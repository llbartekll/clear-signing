import type {DataProviderFfi, TokenMetaFfi} from 'react-native-clear-signing';

const tokenKey = (chainId: bigint, address: string) =>
  `${chainId.toString()}:${address.toLowerCase()}`;

export const SEED_TOKENS = new Map<string, TokenMetaFfi>([
  [
    tokenKey(1n, '0xdac17f958d2ee523a2206206994597c13d831ec7'),
    {symbol: 'USDT', decimals: 6, name: 'Tether USD'},
  ],
  [
    tokenKey(1n, '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48'),
    {symbol: 'USDC', decimals: 6, name: 'USD Coin'},
  ],
  [
    tokenKey(1n, '0x6b175474e89094c44da98b954eedeac495271d0f'),
    {symbol: 'DAI', decimals: 18, name: 'Dai Stablecoin'},
  ],
  [
    tokenKey(1n, '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2'),
    {symbol: 'WETH', decimals: 18, name: 'Wrapped Ether'},
  ],
  [
    tokenKey(1n, '0xae7ab96520de3a18e5e111b5eaab095312d7fe84'),
    {symbol: 'stETH', decimals: 18, name: 'Liquid staked Ether 2.0'},
  ],
  [
    tokenKey(1n, '0xcd5fe23c85820f7b72d0926fc9b05b43e359b7ee'),
    {symbol: 'weETH', decimals: 18, name: 'Wrapped eETH'},
  ],
  [
    tokenKey(1n, '0xe73d53e3a982ab2750a0b76f9012e18b256cc243'),
    {symbol: 'N', decimals: 18, name: 'N'},
  ],
  [
    tokenKey(1n, '0x955d5c14c8d4944da1ea7836bd44d54a8ec35ba1'),
    {symbol: 'RFD', decimals: 18, name: 'Refund Coin'},
  ],
]);

export const SEED_PROXIES = new Map<string, string>();

export class DemoDataProvider implements DataProviderFfi {
  callCount = 0;
  lastQueriedAddress?: string;

  constructor(
    private readonly tokens: Map<string, TokenMetaFfi> = SEED_TOKENS,
    private readonly proxies: Map<string, string> = SEED_PROXIES,
  ) {}

  resolveToken(chainId: bigint, address: string): TokenMetaFfi | undefined {
    return this.tokens.get(tokenKey(chainId, address));
  }
  resolveEnsName(): string | undefined {
    return undefined;
  }
  resolveLocalName(): string | undefined {
    return undefined;
  }
  resolveNftCollectionName(): string | undefined {
    return undefined;
  }
  resolveBlockTimestamp(): bigint | undefined {
    return undefined;
  }
  getImplementationAddress(
    chainId: bigint,
    address: string,
  ): string | undefined {
    this.callCount += 1;
    this.lastQueriedAddress = address.toLowerCase();
    return this.proxies.get(tokenKey(chainId, address));
  }
}
