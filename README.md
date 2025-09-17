
# The Solana Sniper "5N1P3R" - Basic version

# Wprowadzenie

Bot SNIPER to wyspecjalizowane narzędzie transakcyjne dla ekosystemu Solana, zaprojektowane z myślą o maksymalnej prędkości i minimalnym narzucie. Jego architektura jest odzwierciedleniem filozofii "jednego zadania, wykonanego perfekcyjnie". Celem systemu jest ultraszybka reakcja na pojawienie się nowych puli płynności na platformie pump.fun, automatyczny zakup tokena i oddanie dalszej kontroli nad pozycją w ręce operatora.
Projekt jest napisany w języku Rust, wykorzystując asynchroniczne środowisko tokio do obsługi operacji sieciowych, co zapewnia wysoką wydajność i niskie zużycie zasobów. Architektura Komponentowa i Przepływ Danych.

System jest zbudowany w oparciu o modularną architekturę pipeline, gdzie każdy komponent ma jasno zdefiniowaną odpowiedzialność.

1. Sniffer (sniffer.rs, sniffer/) - System Nasłuchu
Funkcjonalność: Jest to "system wczesnego ostrzegania" bota. Jego jedynym zadaniem jest utrzymywanie stałego połączenia WebSocket (WSS) z węzłem RPC Solany i nasłuchiwanie na logi emitowane przez program pump.fun. W przypadku problemów z WSS, posiada mechanizm fallback do odpytywania HTTP.
Relacje: Po zidentyfikowaniu i sparsowaniu logu świadczącego o stworzeniu nowego tokena, Sniffer natychmiast tworzy obiekt PremintCandidate i wysyła go do CandidateBuffer za pośrednictwem asynchronicznego kanału tokio::mpsc.

2. CandidateBuffer (candidate_buffer.rs) - Bufor i Deduplikacja
Funkcjonalność: Działa jak bramka i magazyn krótkoterminowy. Przechowuje napływające kandydatury, natychmiast odrzucając duplikaty (na podstawie adresu mint). Implementuje również mechanizm TTL (Time-To-Live), aby uniknąć przetwarzania przestarzałych sygnałów. Jego kluczową cechą jest zoptymalizowana struktura danych, która pozwala na pobranie najstarszego kandydata w czasie stałym O(1).
Relacje: Odbiera dane ze Sniffera i przekazuje je do BuyEngine.

3. SecurityValidator (security.rs) - Kontrola Bezpieczeństwa
Funkcjonalność: Jest to pierwsza linia obrony przed oczywistym spamem i potencjalnie złośliwymi tokenami. Przeprowadza serię szybkich walidacji, takich jak sprawdzanie, czy klucze publiczne nie są zerowe, czy timestamp tokena jest w rozsądnych granicach i czy nie dochodzi do prób ataku typu "replay" na tę samą sygnaturę.
Relacje: Wywoływany przez BuyEngine do weryfikacji każdego kandydata przed podjęciem próby zakupu.

4. BuyEngine (buy_engine.rs) - Mózg Operacji
Funkcjonalność: Centralna jednostka logiczna bota. Zarządza maszyną stanów (Sniffing vs. PassiveToken). W trybie Sniffing odbiera zweryfikowanych kandydatów, a następnie orkiestruje cały proces zakupu: zleca TransactionBuilder stworzenie transakcji, a następnie przekazuje ją do RpcManager w celu wysłania do sieci. Po udanym zakupie, przechodzi w tryb PassiveToken.
Relacje: Jest centralnym hubem – komunikuje się ze Snifferem (przez CandidateBuffer), SecurityValidator, TransactionBuilder, RpcManager oraz GUI.

5. TransactionBuilder (tx_builder.rs) i Wallet (wallet.rs) - Fabryka Amunicji
Funkcjonalność: TransactionBuilder jest odpowiedzialny za tworzenie surowych, gotowych do wysłania transakcji. Na podstawie PremintCandidate, konstruuje transakcję kupna pump.fun, włączając w to instrukcje ComputeBudget (priority fees). WalletManager wczytuje klucz prywatny operatora z pliku i jest używany przez TransactionBuilder do podpisywania transakcji.
Relacje: BuyEngine zleca TransactionBuilder stworzenie transakcji. TransactionBuilder używa WalletManager do jej podpisania.

6. RpcManager (rpc_manager.rs) - Warstwa Komunikacji Sieciowej
Funkcjonalność: Odpowiada za wysyłanie podpisanych transakcji do sieci Solana. Implementuje strategię broadcastingu do wielu węzłów RPC jednocześnie, aby zmaksymalizować szansę na szybkie przetworzenie. Posiada logikę ponawiania prób i obsługi timeoutów.
Relacje: Odbiera gotowe transakcje od BuyEngine i wysyła je do sieci.

7. GUI (gui.rs) - Panel Kontrolny
Funkcjonalność: Lekki interfejs graficzny, który wizualizuje aktualny stan bota. Jego kluczową funkcją jest udostępnienie operatorowi przycisków (Sell 25%/50%/100%), które pozwalają na ręczną sprzedaż posiadanego tokena.
Relacje: Odczytuje współdzielony stan aplikacji (AppState) i wysyła komendy (GuiEvent) do pętli w main.rs, która następnie wywołuje odpowiednie metody w BuyEngine.

Tryby Pracy
Bot operuje w dwóch głównych trybach, kontrolowanych przez SnifferMode w pliku konfiguracyjnym, oraz w dwóch stanach logicznych zarządzanych przez BuyEngine.
SnifferMode::Real (Tryb Produkcyjny): Bot nasłuchuje na prawdziwe zdarzenia w sieci Solana (mainnet lub devnet, w zależności od konfiguracji RPC).
SnifferMode::Mock (Tryb Testowy): Bot ignoruje sieć i korzysta z wewnętrznego generatora, który emituje fałszywe kandydatury. Idealny do testowania logiki BuyEngine i interfejsu GUI bez ryzyka.
Stan Engine::Sniffing: Stan domyślny. Bot aktywnie poszukuje i analizuje kandydatury. Każdy sygnał z pump.fun jest potencjalnym celem.
Stan Engine::PassiveToken: Stan, w który bot przechodzi po udanym zakupie. W tym stanie bot przestaje reagować na nowe sygnały ze sniffera i przechowuje jeden, konkretny token, czekając na ręczną decyzję o sprzedaży ze strony operatora.
Szacowane Parametry Techniczne i Operacyjne
Poniższe estymacje zakładają uruchomienie bota na dedykowanej maszynie wirtualnej (VPS) zlokalizowanej w centrum danych blisko walidatorów Solana (np. Frankfurt, Niemcy).
Obciążenie Systemu (CPU i RAM)
CPU: ~15-25% obciążenia jednego rdzenia w trybie spoczynku (nasłuch WSS). Skoki do 80-100% jednego rdzenia w momencie wykrycia celu i intensywnego procesu budowania i wysyłania transakcji. Zalecane są co najmniej 2 dedykowane rdzenie.
RAM: 250-500 MB. Zużycie jest stosunkowo niskie dzięki efektywnemu zarządzaniu pamięcią w Rust i braku przechowywania dużych ilości danych historycznych.
Wydajność Transakcyjna (Time-to-Execute)
Jest to czas od momentu wygenerowania logu przez program pump.fun do momentu potwierdzenia transakcji zakupu przez bota.
Scenariusz 1: Darmowe RPC Publiczne
Latencja Sniffera: 50 - 300 ms (wysoka zmienność, kolejkowanie).
Latencja Propagacji TX: 200 - 2000+ ms (transakcja konkuruje z tysiącami innych).
Przewidywany Czas Realizacji: 800 ms - 5 sekund. W warunkach wysokiej konkurencji, skuteczność będzie niska.
Scenariusz 2: Dedykowane RPC (np. Helius, Triton - plan "Pro")
Latencja Sniffera: 5 - 50 ms (niskie opóźnienia, połączenie priorytetowe).
Latencja Propagacji TX: 50 - 400 ms.
Przewidywany Czas Realizacji: 150 ms - 800 ms. Bot staje się konkurencyjny.
Pozycja Konkurencyjna
W obecnej formie, bez integracji z QuantumRaceEngine i Jito, bot jest konkurencyjny wobec innych botów używających standardowych transakcji i publicznych RPC.
Przegra z botami, które używają zaawansowanych strategii, takich jak:
Wysyłanie transakcji bezpośrednio do lidera bloku (TPU).
Używanie serwisów typu Jito do wysyłania "bundle'i" transakcji, które gwarantują atomowe wykonanie i omijają publiczny mempool.
Aby wejść do najwyższej ligi, integracja z Jito lub bezpośrednia komunikacja z TPU jest niezbędna.
Koszty Eksploatacji
Infrastruktura:
VPS: ~$20-40 miesięcznie (np. Hetzner, OVH).
Dedykowany RPC: ~$50-200 miesięcznie (w zależności od planu).
Opłaty Transakcyjne (Fee):
Każda transakcja (zarówno udana, jak i nieudana) ponosi koszt.
Opłata bazowa: ~0.000005 SOL.
Priority Fee (konfigurowalna): Kluczowy element. Agresywna strategia może wymagać 0.001 - 0.01 SOL na transakcję w warunkach wysokiej konkurencji.
Uśredniony koszt udanej transakcji zakupu: Biorąc pod uwagę, że bot wyśle kilka-kilkanaście transakcji równolegle, aby wygrać wyścig, realny koszt jednego udanego zakupu może wynieść 0.01 - 0.05 SOL w opłatach. Do tego dochodzi 1% opłaty pump.fun.


# Wprowadzenie do Testów w Środowisku Symulowanym MarketSimulator®.


Poniższa część dokumentacji definiuje procedurę uruchamiania i analizy testów dla bota 5N1P3R basic, przy użyciu zintegrowanego środowiska MarketSimulator®. Celem jest weryfikacja wydajności, stabilności i logiki biznesowej bota w kontrolowanych, powtarzalnych warunkach, które naśladują dynamikę sieci Solana.

Proces jest w pełni zautomatyzowany przez test_runner, który zarządza całym cyklem życia testu – od budowy komponentów, przez uruchomienie symulacji, aż po agregację wyników i generowanie raportu.


# Faza 1: Przygotowanie Środowiska Testowego

1.1. Wymagania Wstępne

Przed rozpoczęciem upewnij się, że w Twoim środowisku systemowym ($PATH) dostępne są następujące narzędzia:
Rust Toolchain: Niezbędny do kompilacji bota i symulatora (cargo).
Solana CLI: Wymagany jest solana-test-validator do uruchomienia lokalnego klastra.

1.2. Struktura Projektu

Operujemy wewnątrz tego repozytorium. Kluczowe komponenty znajdują się w katalogu main/bot/src:

- main.rs: Główny plik binarny bota.

- src/bin/test_runner.rs: Nasze centrum dowodzenia testami.

- src/bin/market_simulator.rs: Aplikacja symulująca rynek (TokenGenerator + MarketMaker).

- test_config.toml: Plik konfiguracyjny dla test_runnera.

1.3. Konfiguracja Scenariuszy

Testowym, centralnym punktem konfiguracji jest plik: main/bot/test_config.toml. Definiuje on i ustawia przebieg całego testu.

Wymagania:
Ścieżka do solana-test-validator. Musi być w $PATH

Przykład konfiguracji:

solana_test_validator_path = "solana-test-validator"

Ścieżki do crate'ów, które test_runner ma skompilować.

market_simulator_crate_path = "."  # Symulator jest w tym samym crate co runner
sniper_bot_crate_path = "."        # Bot również

Definicja poszczególnych scenariuszy testowych:

[[scenarios]]
name = "Standard Market Conditions"
duration_secs = 120
 ... inne parametry specyficzne dla scenariusza

[[scenarios]]
name = "High-Velocity Gem Rush (Stress Test)"
duration_secs = 60
 ... inne parametry specyficzne dla scenariusza.

Przed uruchomieniem testów, zweryfikuj poprawność ścieżek i dostosuj parametry scenariuszy (np. duration_secs) do swoich potrzeb.


# Faza 2: Uruchomienie Procedury Testowej

Procedura jest w pełni zautomatyzowana. Wszystkie kroki są wykonywane przez test_runner.

2.1. Uruchomienie Test Runnera

Otwórz terminal w głównym katalogu bota (main/bot/) i wykonaj następujące polecenie:

cargo run --release --bin test_runner

To polecenie uruchomi proces testowy. Możesz również zdefiniować własną konfigurację scenariuszy i plik wyjściowy:

cargo run --release --bin test_runner -- --scenarios ./path/to/my_scenarios.toml --output ./path/to/my_results.json.


2.2. Przebieg Zautomatyzowanego Testu

Test Runner wykona następującą sekwencję 6 operacji, dla każdego scenariusza zdefiniowanego w pliku konfiguracyjnym:

1. Kompilacja Binarek: test_runner najpierw skompiluje market_simulator i sniffer_bot_light w trybie --release. Gwarantuje to, że testujemy zoptymalizowany, produkcyjny kod.

2. Uruchomienie Walidatora: W tle zostanie uruchomiony solana-test-validator z flagą --reset, zapewniając czyste, odizolowane środowisko dla każdego testu.

3. Uruchomienie Symulatora i Bota: market_simulator i sniffer_bot_light zostaną uruchomione jako osobne procesy, które natychmiast połączą się z lokalnym walidatorem.

4. Streaming Logów: stdout (w formacie JSON) i stderr obu procesów są na bieżąco zapisywane do dedykowanych plików w katalogu projektu (np. simulator_standard_market_conditions.stdout.jsonl), co zapobiega utracie danych i nadmiernemu zużyciu pamięci.

5. Przebieg Scenariusza: System działa przez zdefiniowany w scenariuszu czas (duration_secs). W tym czasie MarketSimulator generuje tokeny i aktywność rynkową, a SNIPER na nią reaguje.

6. Zakończenie i Sprzątanie: Po upływie czasu, test_runner bezpiecznie zakończy wszystkie procesy (simulator, bot, validator).


# Faza 3: Analiza Wyników

Po zakończeniu wszystkich scenariuszy, test_runner automatycznie przetwarza zebrane logi i generuje finalny raport.

3.1. Raport w Konsoli

Dla każdego scenariusza na konsoli zostanie wyświetlone zwięzłe podsumowanie, zawierające kluczowe metryki wydajności:

--- Test Scenario Summary: 'High-Velocity Gem Rush (Stress Test)' ---
  Tokens Generated:
    - Gem     : 40
    - Rug     : 5
    - Trash   : 5
  Simulator Rug Pulls Executed: 5
  Bot Buy Attempts: 40
  Bot Buy Successes: 38
  Bot Success Rate: 95.00%
  Average Time-to-Execute (TTE): 152.10ms
  P95 Time-to-Execute (TTE): 220.00ms
  Errors Encountered: 2
    - [PROCESSEXITED]: Sniper Bot exited prematurely with status: exit status: 1
